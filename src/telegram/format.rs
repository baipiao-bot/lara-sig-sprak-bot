use std::mem;

use edge_gpt::NewBingResponseMessage;
use regex::Regex;
use teloxide::types::{MessageEntity, MessageEntityKind};

pub fn fix_unordered_list(answer: &mut NewBingResponseMessage) {
    answer.text.insert(0, '\n');
    let re = Regex::new("\n[-]").unwrap();
    answer.text = re.replace_all(&answer.text, "\n•").to_string();
    let re = Regex::new("([.:?!])[-]").unwrap();
    answer.text = re.replace_all(&answer.text, "${1}\n•").to_string();
    answer.text.remove(0);
}

fn sup(mut number: u8) -> String {
    const SUP_CHARACTERS: &str = "⁰¹²³⁴⁵⁶⁷⁸⁹";
    let mut result = String::new();
    let hundred = number / 100;
    if hundred > 0 {
        result.push(SUP_CHARACTERS.chars().nth(hundred as usize).unwrap());
    }
    number %= 100;
    let ten = number / 10;
    if hundred > 0 || ten > 0 {
        result.push(SUP_CHARACTERS.chars().nth(ten as usize).unwrap());
    }
    number %= 10;
    result.push(SUP_CHARACTERS.chars().nth(number as usize).unwrap());
    result
}

struct DetachedMatch {
    start: usize,
    end: usize,
}

impl From<regex::Match<'_>> for DetachedMatch {
    fn from(m: regex::Match) -> Self {
        Self {
            start: m.start(),
            end: m.end(),
        }
    }
}

impl DetachedMatch {
    pub fn as_str<'a>(&self, origin: &'a str) -> &'a str {
        &origin[self.start..self.end]
    }
    pub fn range(&self) -> std::ops::Range<usize> {
        self.start..self.end
    }
}

pub fn fix_attributions(answer: &mut NewBingResponseMessage, entries: &mut Vec<MessageEntity>) {
    let mut text = mem::take(&mut answer.text);
    let re = Regex::new(r"\[\^(\d+)\^\]").unwrap();

    while let Some(m) = re.find(&text).map(DetachedMatch::from) {
        let display_form_attribution_id = m.as_str(&text)[2..m.as_str(&text).len() - 2]
            .parse::<usize>()
            .unwrap();
        let attribution_id = display_form_attribution_id - 1;
        let display_form_attribution_id_sup_str = sup(display_form_attribution_id as _);
        let source_attribution = &answer.source_attributions[attribution_id];
        text.replace_range(m.range(), &display_form_attribution_id_sup_str.to_string());
        let utf16_start = to_utf16_offset(&text, m.start);
        let utf16_size = display_form_attribution_id_sup_str.encode_utf16().count();
        entries.push(MessageEntity {
            offset: utf16_start,
            length: utf16_size,
            kind: MessageEntityKind::TextLink {
                url: source_attribution.parse().unwrap(),
            },
        });
    }
    answer.text = text;
}

pub fn fix_bold(answer: &mut NewBingResponseMessage, entries: &mut Vec<MessageEntity>) {
    let mut text = mem::take(&mut answer.text);
    let re = Regex::new(r"\*\*([^\*]+)\*\*").unwrap();
    while let Some(m) = re.find(&text).map(DetachedMatch::from) {
        let bold_text = m.as_str(&text)[2..m.as_str(&text).len() - 2].to_string();
        text.replace_range(m.range(), &bold_text);
        let utf16_start = to_utf16_offset(&text, m.start);
        let utf16_size = bold_text.encode_utf16().count();
        entries.push(MessageEntity {
            offset: utf16_start,
            length: utf16_size,
            kind: MessageEntityKind::Bold,
        });
    }
    answer.text = text;
}

pub fn to_utf16_offset(s: &str, char_offset: usize) -> usize {
    let origin_utf16_len = s.encode_utf16().count();
    let remaining_utf16_len = s[char_offset..].encode_utf16().count();
    origin_utf16_len - remaining_utf16_len
}
