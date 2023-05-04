use std::env;

use edge_gpt::{ChatSession, ConversationStyle, CookieInFile};
use serde::{Deserialize, Serialize};
use teloxide::{
    payloads::{SendMessage, SendVoice},
    types::{InputFile, MessageEntity, Recipient},
};

use crate::{azure_tts::AzureTTS, duolingo::Vocabulary};
use isolang::Language;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Word {
    spell: String,
    pronunciation: String,
    meaning: String,
    example_sentence: String,
    example_sentence_translation: String,
}

impl Word {
    pub async fn from_vocabulary(
        vocabulary: &Vocabulary,
        ui_language: &str,
        language: &str,
    ) -> Self {
        let spell = &vocabulary.word_string;
        let language_full_name = Language::from_639_1(language).unwrap().to_name();
        let ui_language_full_name = Language::from_639_1(ui_language).unwrap().to_name();
        let promote = format!("look up {language_full_name} word \"{spell}\" in dictionary, output the result in this format: {{\"spell\": \"<word>\", \"pronunciation\": \"<IPA of the word>\", \"meaning\": \"<{ui_language_full_name} meaning>\", \"example_sentence\": \"<Example sentence>\", \"example_sentence_translation\": \"<Example sentence's {ui_language_full_name} meaning>\"}}");
        let mut chat = new_chat().await;
        let result = chat.send_message(&promote).await.unwrap();
        let start_pos = result.text.chars().position(|c| c == '{').unwrap();
        let end_pos = result.text.chars().position(|c| c == '}').unwrap();
        let json_str = &result.text[start_pos..=end_pos];
        serde_json::from_str(json_str).unwrap()
    }

    pub async fn to_telegram_message(
        &self,
        tts: &AzureTTS,
        language: &str,
        chat_id: impl Into<Recipient> + Clone,
    ) -> (SendMessage, SendVoice, SendVoice) {
        let mut text = String::new();
        let mut entities: Vec<MessageEntity> = Vec::new();
        let mut offset = 0;
        let mut add_text = |s: &str, offset: &mut usize| {
            text.push_str(s);
            *offset += s.encode_utf16().count();
        };

        add_text(&self.spell.to_string(), &mut offset);
        entities.push(MessageEntity::bold(0, offset));

        add_text(&format!("\n{}\n", self.pronunciation), &mut offset);

        let meaning_start_offset = offset;
        add_text(&self.meaning.to_string(), &mut offset);
        entities.push(MessageEntity::spoiler(
            meaning_start_offset,
            offset - meaning_start_offset,
        ));

        add_text(&format!("\n{}\n", self.example_sentence), &mut offset);

        let example_sentence_translation_start_offset = offset;
        add_text(&self.example_sentence_translation.to_string(), &mut offset);
        entities.push(MessageEntity::spoiler(
            example_sentence_translation_start_offset,
            offset - example_sentence_translation_start_offset,
        ));
        let mut text_message = SendMessage::new(chat_id.clone(), text);
        text_message.entities = Some(entities);
        let voice = tts
            .voices
            .iter()
            .find(|it| it.locale == language)
            .unwrap()
            .clone();
        let spell_voice = tts.tts_simple(&self.spell, &voice);
        let sentence_voice = tts.tts_simple(&self.example_sentence, &voice);
        let (spell_voice, sentence_voice) = tokio::join!(spell_voice, sentence_voice);
        let spell_file = InputFile::memory(spell_voice).file_name("spell.ogg");
        let sentence_file = InputFile::memory(sentence_voice).file_name("sentence.ogg");
        let mut spell_voice = SendVoice::new(chat_id.clone(), spell_file);
        spell_voice.disable_notification = Some(true);
        let mut sentence_voice = SendVoice::new(chat_id, sentence_file);
        sentence_voice.disable_notification = Some(true);
        (text_message, spell_voice, sentence_voice)
    }
}

async fn new_chat() -> ChatSession {
    let cookie_str = env::var("EDGE_GPT_COOKIE").unwrap();
    let cookies: Vec<CookieInFile> = serde_json::from_str(&cookie_str).unwrap();
    ChatSession::create(ConversationStyle::Balanced, &cookies)
        .await
        .unwrap()
}
