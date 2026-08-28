use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexCompileError {
    message: String,
}

impl RegexCompileError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for RegexCompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for RegexCompileError {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RegexOptions {
    pub case_sensitive: bool,
    pub multi_line: bool,
    pub dot_matches_new_line: bool,
    pub ignore_whitespace: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
pub struct SimpleRegex {
    pattern: Vec<char>,
    options: RegexOptions,
}

#[derive(Debug, Clone)]
enum Atom {
    Literal(char),
    Dot,
    Class(CharClass),
    Group { start: usize, end: usize },
    AnchorStart,
    AnchorEnd,
}

#[derive(Debug, Clone, Copy)]
struct Quantifier {
    min: usize,
    max: Option<usize>,
}

#[derive(Debug, Clone)]
struct CharClass {
    negated: bool,
    items: Vec<ClassItem>,
}

#[derive(Debug, Clone)]
enum ClassItem {
    Char(char),
    Range(char, char),
    Digit,
    Word,
    Space,
}

struct IndexedText<'a> {
    haystack: &'a str,
    chars: Vec<char>,
    byte_offsets: Vec<usize>,
}

impl SimpleRegex {
    pub fn compile(pattern: &str, options: RegexOptions) -> Result<Self, RegexCompileError> {
        let pattern = if options.ignore_whitespace {
            strip_extended_whitespace(pattern)
        } else {
            pattern.to_owned()
        };
        let regex = Self {
            pattern: pattern.chars().collect(),
            options,
        };
        regex.validate_range(0, regex.pattern.len())?;
        Ok(regex)
    }

    pub fn is_match(&self, haystack: &str) -> bool {
        !self.find_iter(haystack).is_empty()
    }

    pub fn find_iter(&self, haystack: &str) -> Vec<MatchSpan> {
        let text = IndexedText::new(haystack);
        let mut matches = Vec::new();
        let mut start = 0usize;

        while start <= text.chars.len() {
            let mut ends = self.match_expr(0, self.pattern.len(), &text, start);
            ends.retain(|end| *end >= start);
            ends.sort_unstable_by(|left, right| right.cmp(left));
            ends.dedup();

            if let Some(end) = ends.first().copied() {
                matches.push(MatchSpan {
                    start: text.byte_at(start),
                    end: text.byte_at(end),
                });
                start = if end > start { end } else { start + 1 };
            } else {
                start += 1;
            }
        }

        matches
    }

    fn validate_range(&self, start: usize, end: usize) -> Result<(), RegexCompileError> {
        let mut pos = start;
        while pos < end {
            match self.pattern[pos] {
                '\\' => {
                    pos += 2;
                }
                '[' => {
                    pos = self.class_end(pos, end)? + 1;
                }
                '(' => {
                    let group_start = if self.pattern.get(pos + 1) == Some(&'?')
                        && self.pattern.get(pos + 2) == Some(&':')
                    {
                        pos + 3
                    } else {
                        pos + 1
                    };
                    if self.pattern.get(pos + 1) == Some(&'?') && group_start != pos + 3 {
                        return Err(self.err(pos, "unsupported group extension; use (?:...)"));
                    }
                    let group_end = self.group_end(pos, end)?;
                    self.validate_range(group_start, group_end)?;
                    pos = group_end + 1;
                }
                ')' | ']' => {
                    return Err(self.err(pos, "unmatched closing delimiter"));
                }
                _ => {
                    pos += 1;
                }
            }
        }
        Ok(())
    }

    fn match_expr(
        &self,
        start: usize,
        end: usize,
        text: &IndexedText<'_>,
        text_pos: usize,
    ) -> Vec<usize> {
        let mut branch_start = start;
        let mut results = Vec::new();

        for split in self.top_level_splits(start, end) {
            results.extend(self.match_sequence(branch_start, split, text, text_pos));
            branch_start = split + 1;
        }
        results.extend(self.match_sequence(branch_start, end, text, text_pos));
        dedup_descending(results)
    }

    fn match_sequence(
        &self,
        mut pos: usize,
        end: usize,
        text: &IndexedText<'_>,
        text_pos: usize,
    ) -> Vec<usize> {
        if pos >= end {
            return vec![text_pos];
        }

        let (atom, next_pos) = match self.parse_atom(pos, end) {
            Ok(value) => value,
            Err(_) => return Vec::new(),
        };
        pos = next_pos;

        let (quantifier, after_quantifier) = match self.parse_quantifier(pos, end) {
            Ok(value) => value,
            Err(_) => return Vec::new(),
        };

        let mut results = Vec::new();
        for atom_end in self.match_quantified(&atom, quantifier, text, text_pos) {
            results.extend(self.match_sequence(after_quantifier, end, text, atom_end));
        }
        dedup_descending(results)
    }

    fn match_quantified(
        &self,
        atom: &Atom,
        quantifier: Quantifier,
        text: &IndexedText<'_>,
        text_pos: usize,
    ) -> Vec<usize> {
        let mut out = Vec::new();
        self.repeat_atom(atom, quantifier, text, text_pos, 0, &mut out);
        dedup_descending(out)
    }

    fn repeat_atom(
        &self,
        atom: &Atom,
        quantifier: Quantifier,
        text: &IndexedText<'_>,
        text_pos: usize,
        count: usize,
        out: &mut Vec<usize>,
    ) {
        if count >= quantifier.min {
            out.push(text_pos);
        }
        if quantifier.max.is_some_and(|max| count >= max) {
            return;
        }

        for next in self.match_atom_once(atom, text, text_pos) {
            if next == text_pos {
                continue;
            }
            self.repeat_atom(atom, quantifier, text, next, count + 1, out);
        }
    }

    fn match_atom_once(&self, atom: &Atom, text: &IndexedText<'_>, pos: usize) -> Vec<usize> {
        match atom {
            Atom::Literal(expected) => text
                .chars
                .get(pos)
                .copied()
                .filter(|actual| chars_equal(*expected, *actual, self.options.case_sensitive))
                .map(|_| vec![pos + 1])
                .unwrap_or_default(),
            Atom::Dot => text
                .chars
                .get(pos)
                .copied()
                .filter(|ch| self.options.dot_matches_new_line || (*ch != '\n' && *ch != '\r'))
                .map(|_| vec![pos + 1])
                .unwrap_or_default(),
            Atom::Class(class) => text
                .chars
                .get(pos)
                .copied()
                .filter(|ch| class.matches(*ch, self.options.case_sensitive))
                .map(|_| vec![pos + 1])
                .unwrap_or_default(),
            Atom::Group { start, end } => self.match_expr(*start, *end, text, pos),
            Atom::AnchorStart => {
                if pos == 0
                    || (self.options.multi_line
                        && pos > 0
                        && text.chars.get(pos - 1) == Some(&'\n'))
                {
                    vec![pos]
                } else {
                    Vec::new()
                }
            }
            Atom::AnchorEnd => {
                if pos == text.chars.len()
                    || text.chars.get(pos) == Some(&'\n')
                    || (text.chars.get(pos) == Some(&'\r')
                        && text.chars.get(pos + 1) == Some(&'\n'))
                {
                    vec![pos]
                } else {
                    Vec::new()
                }
            }
        }
    }

    fn parse_atom(&self, pos: usize, end: usize) -> Result<(Atom, usize), RegexCompileError> {
        if pos >= end {
            return Err(self.err(pos, "expected atom"));
        }

        match self.pattern[pos] {
            '(' => {
                let group_start = if self.pattern.get(pos + 1) == Some(&'?')
                    && self.pattern.get(pos + 2) == Some(&':')
                {
                    pos + 3
                } else {
                    pos + 1
                };
                let group_end = self.group_end(pos, end)?;
                Ok((
                    Atom::Group {
                        start: group_start,
                        end: group_end,
                    },
                    group_end + 1,
                ))
            }
            '[' => {
                let class_end = self.class_end(pos, end)?;
                Ok((
                    Atom::Class(self.parse_class(pos + 1, class_end)?),
                    class_end + 1,
                ))
            }
            '\\' => self.parse_escape(pos, end),
            '.' => Ok((Atom::Dot, pos + 1)),
            '^' => Ok((Atom::AnchorStart, pos + 1)),
            '$' => Ok((Atom::AnchorEnd, pos + 1)),
            '*' | '+' | '?' => Err(self.err(pos, "quantifier has nothing to repeat")),
            ch => Ok((Atom::Literal(ch), pos + 1)),
        }
    }

    fn parse_escape(&self, pos: usize, end: usize) -> Result<(Atom, usize), RegexCompileError> {
        let Some(ch) = self.pattern.get(pos + 1).copied() else {
            return Err(self.err(pos, "dangling escape"));
        };
        if pos + 1 >= end {
            return Err(self.err(pos, "dangling escape"));
        }

        let atom = match ch {
            'd' => Atom::Class(CharClass::single(ClassItem::Digit, false)),
            'D' => Atom::Class(CharClass::single(ClassItem::Digit, true)),
            'w' => Atom::Class(CharClass::single(ClassItem::Word, false)),
            'W' => Atom::Class(CharClass::single(ClassItem::Word, true)),
            's' => Atom::Class(CharClass::single(ClassItem::Space, false)),
            'S' => Atom::Class(CharClass::single(ClassItem::Space, true)),
            'n' => Atom::Literal('\n'),
            'r' => Atom::Literal('\r'),
            't' => Atom::Literal('\t'),
            ch => Atom::Literal(ch),
        };
        Ok((atom, pos + 2))
    }

    fn parse_quantifier(
        &self,
        pos: usize,
        end: usize,
    ) -> Result<(Quantifier, usize), RegexCompileError> {
        if pos >= end {
            return Ok((Quantifier::exactly_one(), pos));
        }

        let (quantifier, mut next) = match self.pattern[pos] {
            '*' => (Quantifier { min: 0, max: None }, pos + 1),
            '+' => (Quantifier { min: 1, max: None }, pos + 1),
            '?' => (
                Quantifier {
                    min: 0,
                    max: Some(1),
                },
                pos + 1,
            ),
            '{' => self.parse_braced_quantifier(pos, end)?,
            _ => return Ok((Quantifier::exactly_one(), pos)),
        };

        if self.pattern.get(next) == Some(&'?') {
            next += 1;
        }
        Ok((quantifier, next))
    }

    fn parse_braced_quantifier(
        &self,
        pos: usize,
        end: usize,
    ) -> Result<(Quantifier, usize), RegexCompileError> {
        let mut cursor = pos + 1;
        let min = self.parse_number(&mut cursor, end)?;
        let max = if self.pattern.get(cursor) == Some(&',') {
            cursor += 1;
            if self.pattern.get(cursor) == Some(&'}') {
                None
            } else {
                Some(self.parse_number(&mut cursor, end)?)
            }
        } else {
            Some(min)
        };
        if self.pattern.get(cursor) != Some(&'}') {
            return Err(self.err(pos, "unclosed braced quantifier"));
        }
        if max.is_some_and(|max| max < min) {
            return Err(self.err(pos, "quantifier maximum is smaller than minimum"));
        }
        Ok((Quantifier { min, max }, cursor + 1))
    }

    fn parse_number(&self, cursor: &mut usize, end: usize) -> Result<usize, RegexCompileError> {
        let start = *cursor;
        while *cursor < end && self.pattern[*cursor].is_ascii_digit() {
            *cursor += 1;
        }
        if start == *cursor {
            return Err(self.err(start, "expected quantifier number"));
        }
        self.pattern[start..*cursor]
            .iter()
            .collect::<String>()
            .parse()
            .map_err(|_| self.err(start, "invalid quantifier number"))
    }

    fn parse_class(&self, start: usize, end: usize) -> Result<CharClass, RegexCompileError> {
        let mut cursor = start;
        let negated = self.pattern.get(cursor) == Some(&'^');
        if negated {
            cursor += 1;
        }

        let mut items = Vec::new();
        while cursor < end {
            let (first, next) = self.class_atom(cursor, end)?;
            cursor = next;

            if let ClassItem::Char(start_ch) = first.clone() {
                if self.pattern.get(cursor) == Some(&'-') && cursor + 1 < end {
                    cursor += 1;
                    let (second, after_second) = self.class_atom(cursor, end)?;
                    cursor = after_second;
                    if let ClassItem::Char(end_ch) = second {
                        items.push(ClassItem::Range(start_ch, end_ch));
                    } else {
                        return Err(
                            self.err(cursor, "character class range must end with a character")
                        );
                    }
                    continue;
                }
            }

            items.push(first);
        }

        Ok(CharClass { negated, items })
    }

    fn class_atom(&self, pos: usize, end: usize) -> Result<(ClassItem, usize), RegexCompileError> {
        if pos >= end {
            return Err(self.err(pos, "unexpected end of character class"));
        }
        if self.pattern[pos] != '\\' {
            return Ok((ClassItem::Char(self.pattern[pos]), pos + 1));
        }
        let Some(ch) = self.pattern.get(pos + 1).copied() else {
            return Err(self.err(pos, "dangling escape in character class"));
        };

        let item = match ch {
            'd' => ClassItem::Digit,
            'w' => ClassItem::Word,
            's' => ClassItem::Space,
            'n' => ClassItem::Char('\n'),
            'r' => ClassItem::Char('\r'),
            't' => ClassItem::Char('\t'),
            ch => ClassItem::Char(ch),
        };
        Ok((item, pos + 2))
    }

    fn top_level_splits(&self, start: usize, end: usize) -> Vec<usize> {
        let mut splits = Vec::new();
        let mut pos = start;
        while pos < end {
            match self.pattern[pos] {
                '\\' => pos += 2,
                '[' => pos = self.class_end(pos, end).map_or(end, |index| index + 1),
                '(' => pos = self.group_end(pos, end).map_or(end, |index| index + 1),
                '|' => {
                    splits.push(pos);
                    pos += 1;
                }
                _ => pos += 1,
            }
        }
        splits
    }

    fn group_end(&self, start: usize, end: usize) -> Result<usize, RegexCompileError> {
        let mut depth = 0usize;
        let mut pos = start;
        while pos < end {
            match self.pattern[pos] {
                '\\' => pos += 2,
                '[' => pos = self.class_end(pos, end)? + 1,
                '(' => {
                    depth += 1;
                    pos += 1;
                }
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(pos);
                    }
                    pos += 1;
                }
                _ => pos += 1,
            }
        }
        Err(self.err(start, "unclosed group"))
    }

    fn class_end(&self, start: usize, end: usize) -> Result<usize, RegexCompileError> {
        let mut pos = start + 1;
        let mut first = true;
        while pos < end {
            match self.pattern[pos] {
                '\\' => pos += 2,
                ']' if !first => return Ok(pos),
                _ => {
                    first = false;
                    pos += 1;
                }
            }
        }
        Err(self.err(start, "unclosed character class"))
    }

    fn err(&self, pos: usize, message: impl Into<String>) -> RegexCompileError {
        RegexCompileError::new(format!(
            "regex parse error at character {pos}: {}",
            message.into()
        ))
    }
}

impl Quantifier {
    fn exactly_one() -> Self {
        Self {
            min: 1,
            max: Some(1),
        }
    }
}

impl CharClass {
    fn single(item: ClassItem, negated: bool) -> Self {
        Self {
            negated,
            items: vec![item],
        }
    }

    fn matches(&self, ch: char, case_sensitive: bool) -> bool {
        let matched = self
            .items
            .iter()
            .any(|item| item.matches(ch, case_sensitive));
        if self.negated {
            !matched
        } else {
            matched
        }
    }
}

impl ClassItem {
    fn matches(&self, ch: char, case_sensitive: bool) -> bool {
        match self {
            ClassItem::Char(expected) => chars_equal(*expected, ch, case_sensitive),
            ClassItem::Range(start, end) => {
                let ch = comparable_char(ch, case_sensitive);
                let start = comparable_char(*start, case_sensitive);
                let end = comparable_char(*end, case_sensitive);
                start <= ch && ch <= end
            }
            ClassItem::Digit => ch.is_ascii_digit(),
            ClassItem::Word => ch.is_ascii_alphanumeric() || ch == '_',
            ClassItem::Space => ch.is_whitespace(),
        }
    }
}

impl<'a> IndexedText<'a> {
    fn new(haystack: &'a str) -> Self {
        let mut chars = Vec::new();
        let mut byte_offsets = Vec::new();
        for (offset, ch) in haystack.char_indices() {
            byte_offsets.push(offset);
            chars.push(ch);
        }
        Self {
            haystack,
            chars,
            byte_offsets,
        }
    }

    fn byte_at(&self, char_index: usize) -> usize {
        self.byte_offsets
            .get(char_index)
            .copied()
            .unwrap_or(self.haystack.len())
    }
}

fn dedup_descending(mut values: Vec<usize>) -> Vec<usize> {
    values.sort_unstable_by(|left, right| right.cmp(left));
    values.dedup();
    values
}

fn chars_equal(left: char, right: char, case_sensitive: bool) -> bool {
    left == right
        || (!case_sensitive
            && left
                .to_lowercase()
                .collect::<String>()
                .eq(&right.to_lowercase().collect::<String>()))
}

fn comparable_char(ch: char, case_sensitive: bool) -> char {
    if case_sensitive {
        ch
    } else {
        ch.to_ascii_lowercase()
    }
}

fn strip_extended_whitespace(pattern: &str) -> String {
    let mut out = String::new();
    let mut escaped = false;
    let mut in_class = false;
    let mut chars = pattern.chars().peekable();

    while let Some(ch) = chars.next() {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' => {
                out.push(ch);
                escaped = true;
            }
            '[' => {
                in_class = true;
                out.push(ch);
            }
            ']' => {
                in_class = false;
                out.push(ch);
            }
            '#' if !in_class => while chars.next().is_some_and(|comment_ch| comment_ch != '\n') {},
            ch if !in_class && ch.is_whitespace() => {}
            ch => out.push(ch),
        }
    }

    out
}
