//! Small dependency-free JSON value/parser used by the `.eeproj` manifest.
//! The parser implements the JSON grammar (including UTF-16 surrogate pairs)
//! rather than the permissive subset in the initial prototype.

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
        match self {
            Json::Obj(entries) => entries
                .iter_mut()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        let value = self.as_f64()?;
        if value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= u64::MAX as f64 {
            Some(value as u64)
        } else {
            None
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(values) => Some(values),
            _ => None,
        }
    }

    pub fn as_obj(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Obj(values) => Some(values),
            _ => None,
        }
    }

    pub fn write(&self) -> String {
        self.write_checked()
            .expect("JSON values must contain finite numbers")
    }

    pub fn write_checked(&self) -> Result<String, String> {
        let mut output = String::new();
        self.write_to(&mut output)?;
        Ok(output)
    }

    fn write_to(&self, output: &mut String) -> Result<(), String> {
        match self {
            Json::Null => output.push_str("null"),
            Json::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
            Json::Num(value) => {
                if !value.is_finite() {
                    return Err("JSON numbers must be finite".into());
                }
                output.push_str(&format_num(*value));
            }
            Json::Str(value) => write_json_string(value, output),
            Json::Arr(items) => {
                output.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    item.write_to(output)?;
                }
                output.push(']');
            }
            Json::Obj(entries) => {
                output.push('{');
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    write_json_string(key, output);
                    output.push(':');
                    value.write_to(output)?;
                }
                output.push('}');
            }
        }
        Ok(())
    }
}

fn format_num(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn write_json_string(value: &str, output: &mut String) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if (character as u32) < 0x20 => {
                output.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

pub fn parse(input: &str) -> Result<Json, String> {
    let characters: Vec<char> = input.chars().collect();
    let mut parser = Parser {
        source: &characters,
        index: 0,
    };
    parser.skip_whitespace();
    let value = parser.value()?;
    parser.skip_whitespace();
    if parser.index != parser.source.len() {
        return Err(format!("trailing data at character {}", parser.index));
    }
    Ok(value)
}

struct Parser<'a> {
    source: &'a [char],
    index: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> {
        self.source.get(self.index).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.index += 1;
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), String> {
        if self.peek() == Some(expected) {
            self.index += 1;
            Ok(())
        } else {
            Err(format!("expected '{expected}' at character {}", self.index))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.skip_whitespace();
        match self.peek() {
            Some('{') => self.object(),
            Some('[') => self.array(),
            Some('"') => Ok(Json::Str(self.string()?)),
            Some('t') => {
                self.literal("true")?;
                Ok(Json::Bool(true))
            }
            Some('f') => {
                self.literal("false")?;
                Ok(Json::Bool(false))
            }
            Some('n') => {
                self.literal("null")?;
                Ok(Json::Null)
            }
            Some('-' | '0'..='9') => self.number(),
            other => Err(format!("unexpected {other:?} at character {}", self.index)),
        }
    }

    fn literal(&mut self, literal: &str) -> Result<(), String> {
        for expected in literal.chars() {
            if self.peek() != Some(expected) {
                return Err(format!("invalid literal at character {}", self.index));
            }
            self.index += 1;
        }
        Ok(())
    }

    fn object(&mut self) -> Result<Json, String> {
        self.expect('{')?;
        let mut entries: Vec<(String, Json)> = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some('}') {
            self.index += 1;
            return Ok(Json::Obj(entries));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some('"') {
                return Err(format!(
                    "object key must be a string at character {}",
                    self.index
                ));
            }
            let key = self.string()?;
            if entries.iter().any(|(existing, _)| existing == &key) {
                return Err(format!("duplicate object key '{key}'"));
            }
            self.skip_whitespace();
            self.expect(':')?;
            let value = self.value()?;
            entries.push((key, value));
            self.skip_whitespace();
            match self.peek() {
                Some(',') => {
                    self.index += 1;
                    self.skip_whitespace();
                    if self.peek() == Some('}') {
                        return Err("trailing comma in object".into());
                    }
                }
                Some('}') => {
                    self.index += 1;
                    return Ok(Json::Obj(entries));
                }
                other => return Err(format!("expected ',' or '}}', got {other:?}")),
            }
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.expect('[')?;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(']') {
            self.index += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_whitespace();
            match self.peek() {
                Some(',') => {
                    self.index += 1;
                    self.skip_whitespace();
                    if self.peek() == Some(']') {
                        return Err("trailing comma in array".into());
                    }
                }
                Some(']') => {
                    self.index += 1;
                    return Ok(Json::Arr(items));
                }
                other => return Err(format!("expected ',' or ']', got {other:?}")),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect('"')?;
        let mut output = String::new();
        loop {
            match self.peek() {
                None => return Err("unterminated string".into()),
                Some('"') => {
                    self.index += 1;
                    return Ok(output);
                }
                Some('\\') => {
                    self.index += 1;
                    let escaped = self.peek().ok_or("unterminated escape sequence")?;
                    self.index += 1;
                    match escaped {
                        '"' => output.push('"'),
                        '\\' => output.push('\\'),
                        '/' => output.push('/'),
                        'b' => output.push('\u{08}'),
                        'f' => output.push('\u{0c}'),
                        'n' => output.push('\n'),
                        'r' => output.push('\r'),
                        't' => output.push('\t'),
                        'u' => {
                            let first = self.hex_quad()?;
                            let codepoint = if (0xD800..=0xDBFF).contains(&first) {
                                if self.peek() != Some('\\')
                                    || self.source.get(self.index + 1) != Some(&'u')
                                {
                                    return Err(
                                        "high surrogate must be followed by a low surrogate".into(),
                                    );
                                }
                                self.index += 2;
                                let second = self.hex_quad()?;
                                if !(0xDC00..=0xDFFF).contains(&second) {
                                    return Err("invalid low surrogate".into());
                                }
                                0x10000
                                    + (((first - 0xD800) as u32) << 10)
                                    + (second - 0xDC00) as u32
                            } else if (0xDC00..=0xDFFF).contains(&first) {
                                return Err("unexpected low surrogate".into());
                            } else {
                                first as u32
                            };
                            output.push(char::from_u32(codepoint).ok_or("invalid Unicode escape")?);
                        }
                        other => return Err(format!("invalid escape '\\{other}'")),
                    }
                }
                Some(character) if (character as u32) < 0x20 => {
                    return Err("unescaped control character in string".into());
                }
                Some(character) => {
                    output.push(character);
                    self.index += 1;
                }
            }
        }
    }

    fn hex_quad(&mut self) -> Result<u16, String> {
        let mut value = 0_u16;
        for _ in 0..4 {
            let digit = self
                .peek()
                .and_then(|character| character.to_digit(16))
                .ok_or("invalid Unicode escape")?;
            value = (value << 4) | digit as u16;
            self.index += 1;
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.index;
        if self.peek() == Some('-') {
            self.index += 1;
        }
        match self.peek() {
            Some('0') => {
                self.index += 1;
                if matches!(self.peek(), Some('0'..='9')) {
                    return Err(format!(
                        "leading zero in number at character {}",
                        self.index
                    ));
                }
            }
            Some('1'..='9') => {
                while matches!(self.peek(), Some('0'..='9')) {
                    self.index += 1;
                }
            }
            _ => return Err(format!("invalid number at character {}", self.index)),
        }
        if self.peek() == Some('.') {
            self.index += 1;
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err("fraction requires at least one digit".into());
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.index += 1;
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.index += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.index += 1;
            }
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err("exponent requires at least one digit".into());
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.index += 1;
            }
        }
        let text: String = self.source[start..self.index].iter().collect();
        let value = text.parse::<f64>().map_err(|error| error.to_string())?;
        if !value.is_finite() {
            return Err("number is outside the finite f64 range".into());
        }
        Ok(Json::Num(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_nested_values_and_escapes() {
        let source = r#"{"name":"EEE \"Workbench\"","enabled":true,"data":[1,-2.5e-3,null,"line\nnext","\ud83d\udd27"]}"#;
        let parsed = parse(source).unwrap();
        let encoded = parsed.write();
        assert_eq!(parse(&encoded).unwrap(), parsed);
        assert_eq!(
            parsed.get("name").and_then(Json::as_str),
            Some("EEE \"Workbench\"")
        );
    }

    #[test]
    fn rejects_invalid_numbers_and_trailing_commas() {
        for invalid in ["01", "1.", ".2", "1e", "[1,]", "{\"x\":1,}"] {
            assert!(parse(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn rejects_duplicate_keys_and_invalid_surrogates() {
        assert!(parse(r#"{"x":1,"x":2}"#).is_err());
        assert!(parse(r#""\ud800""#).is_err());
    }
}
