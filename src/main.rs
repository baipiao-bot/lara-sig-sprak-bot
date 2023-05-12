mod azure_tts;
mod bing_dictionary;
mod duolingo;
mod telegram;
mod util;

use bing_dictionary::Word;
use bytes::Bytes;
use edge_gpt::{ChatSession, ConversationStyle, CookieInFile, NewBingResponseMessage};
use ezio::prelude::*;
use rand::prelude::*;
use redis::{aio::Connection, AsyncCommands};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::env;
use telegram::{
    fix_attributions, fix_bold, fix_unordered_list, simple_respond_message, to_utf16_offset,
};
use teloxide::{
    payloads::SendMessage,
    types::{Message, MessageEntity, MessageEntityKind, Update, UpdateKind},
};
use util::decrypt;
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    Start,
    DuolingoLogin,
    RandomWord,
    Chat,
    Story,
    Help,
}

impl TryFrom<&str> for CommandKind {
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "start" => Ok(Self::Start),
            "duolingo_login" => Ok(Self::DuolingoLogin),
            "random_word" => Ok(Self::RandomWord),
            "chat" => Ok(Self::Chat),
            "story" => Ok(Self::Story),
            "help" => Ok(Self::Help),
            _ => Err(()),
        }
    }

    type Error = ();
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Bot {
    #[serde(default = "telegram::Telegram::from_env", skip_serializing)]
    pub telegram: telegram::Telegram,
    pub azure_tts: azure_tts::AzureTTS,
    pub duolingo: Option<duolingo::Duolingo>,
}

impl Bot {
    pub async fn new(
        telegram_token: impl ToString,
        azure_tts_subscription_key: impl ToString,
    ) -> Self {
        Self {
            telegram: telegram::Telegram::new(telegram_token),
            azure_tts: azure_tts::AzureTTS::new(azure_tts_subscription_key).await,
            duolingo: None,
        }
    }

    pub async fn handle(&mut self, message: &Message, redis_connection: &mut Connection) {
        if let Some(text) = message.text() {
            if text.starts_with('/') {
                let end_of_command_text =
                    text.chars().position(|it| it == ' ').unwrap_or(text.len());
                let command_str = &text[1..end_of_command_text];
                let params_str = &text[end_of_command_text..];
                if let Ok(command) = command_str.try_into() {
                    match command {
                        CommandKind::Start => {
                            let respond = simple_respond_message(
                                message,
                                "Hello, this is a bot for language learning.\n Try `/help` to see what I can do.",
                            );
                            self.telegram.send_message(&respond).await;
                        }
                        CommandKind::DuolingoLogin => {
                            let mut params = params_str.trim().split(' ');
                            let name = params.next().unwrap();
                            let jwt = params.next().unwrap();
                            let duolingo = duolingo::Duolingo::new(name, jwt).await;
                            self.duolingo = Some(duolingo);
                        }
                        CommandKind::RandomWord => {
                            self.random_word(message).await;
                        }
                        CommandKind::Chat => {
                            self.start_chat(message, redis_connection).await;
                        }
                        CommandKind::Story => {
                            self.story(message).await;
                        }
                        _ => {
                            unimplemented!()
                        }
                    }
                }
            } else if message.reply_to_message().is_some() {
                self.response_chat(message, redis_connection).await;
            }
        }
    }

    async fn random_word(&mut self, message: &Message) {
        if let Some(duolingo) = &self.duolingo {
            let vocabulary = {
                let mut rng = thread_rng();
                duolingo.vocabulary.choose(&mut rng).unwrap()
            };
            let language = duolingo.languages.first().unwrap();
            let status_sender = self.telegram.start_sending_typing_status(message.chat.id);
            let word =
                Word::from_vocabulary(vocabulary, duolingo.ui_language.as_ref(), language).await;
            let (text, word, sentence) = word
                .to_telegram_message(&self.azure_tts, language, message.chat.id)
                .await;
            status_sender.send(()).unwrap();
            self.telegram.send_message(&text).await;
            self.telegram.send_voice(message.chat.id, &word).await;
            self.telegram.send_voice(message.chat.id, &sentence).await;
        } else {
            let respond = simple_respond_message(
                message,
                "Please use `/duolingo_login` to login to duolingo.",
            );
            self.telegram.send_message(&respond).await;
        }
    }

    async fn chat_respond_from_bing(
        &self,
        message: &Message,
        mut bing_respond: NewBingResponseMessage,
    ) -> (SendMessage, Bytes) {
        let mut entities = Vec::new();
        fix_unordered_list(&mut bing_respond);
        fix_attributions(&mut bing_respond, &mut entities);
        fix_bold(&mut bing_respond, &mut entities);
        if let Some(duolingo) = &self.duolingo {
            let language = duolingo.languages.first().unwrap();
            let translation_hided = hide_translation(&bing_respond, &mut entities);
            let tts_content = extract_tts_part(&translation_hided);
            let voice = self
                .azure_tts
                .voices
                .iter()
                .find(|it| it.locale.contains(language))
                .unwrap()
                .clone();
            let tts_result = self.azure_tts.tts_simple(&tts_content, &voice).await;
            (
                SendMessage {
                    chat_id: message.chat.id.into(),
                    text: bing_respond.text,
                    entities: Some(entities),
                    disable_web_page_preview: Some(true),
                    reply_to_message_id: Some(message.id),
                    message_thread_id: None,
                    parse_mode: None,
                    disable_notification: None,
                    protect_content: None,
                    allow_sending_without_reply: None,
                    reply_markup: None,
                },
                tts_result,
            )
        } else {
            unimplemented!()
        }
    }

    async fn story_respond_from_bing(
        &self,
        message: &Message,
        bing_respond: NewBingResponseMessage,
    ) -> (SendMessage, Bytes) {
        if let Some(duolingo) = &self.duolingo {
            let language = duolingo.languages.first().unwrap();
            let start_position = bing_respond.text.find("\"\"\"").unwrap();
            let end_position = bing_respond
                .text
                .rfind("\"\"\"")
                .unwrap_or(bing_respond.text.len());
            let content = bing_respond.text[start_position + 3..end_position].trim();
            let voice = self
                .azure_tts
                .voices
                .iter()
                .find(|it| it.locale.contains(language))
                .unwrap()
                .clone();
            let tts_result = self.azure_tts.tts_simple(content, &voice).await;
            (
                SendMessage {
                    chat_id: message.chat.id.into(),
                    text: bing_respond.text,
                    entities: None,
                    disable_web_page_preview: Some(true),
                    reply_to_message_id: Some(message.id),
                    message_thread_id: None,
                    parse_mode: None,
                    disable_notification: None,
                    protect_content: None,
                    allow_sending_without_reply: None,
                    reply_markup: None,
                },
                tts_result,
            )
        } else {
            unimplemented!()
        }
    }

    async fn start_chat(&self, message: &Message, redis_connection: &mut Connection) {
        let cookie_str = env::var("EDGE_GPT_COOKIE").unwrap();
        let cookies: Vec<CookieInFile> = serde_json::from_str(&cookie_str).unwrap();
        let status_sender = self.telegram.start_sending_typing_status(message.chat.id);
        let mut session = ChatSession::create(ConversationStyle::Creative, &cookies)
            .await
            .unwrap();
        let response = session
            .send_message(include_str!("../chat_promote.txt"))
            .await
            .unwrap();
        let (send_message, tts_result) = self.chat_respond_from_bing(message, response).await;
        status_sender.send(()).unwrap();
        let send_message_response = self.telegram.send_message(&send_message).await;
        self.telegram.send_voice(message.chat.id, &tts_result).await;
        let key = format!("{}-{}", message.chat.id, send_message_response.id);
        let session_str = serde_json::to_string(&session).unwrap();
        let _: () = redis_connection
            .set_ex(key, session_str, 60 * 60)
            .await
            .unwrap();
    }

    async fn response_chat(&self, message: &Message, redis_connection: &mut Connection) {
        let reply_to_message = message.reply_to_message().unwrap();
        let key = format!("{}-{}", message.chat.id, reply_to_message.id);
        let corresponding_session: String = redis_connection.get(key).await.unwrap();
        let mut session: ChatSession = serde_json::from_str(&corresponding_session).unwrap();
        let status_sender = self.telegram.start_sending_typing_status(message.chat.id);
        let response = session.send_message(message.text().unwrap()).await.unwrap();
        let (send_message, tts_result) = self.chat_respond_from_bing(message, response).await;
        status_sender.send(()).unwrap();
        let send_message_response = self.telegram.send_message(&send_message).await;
        self.telegram.send_voice(message.chat.id, &tts_result).await;
        let key = format!("{}-{}", message.chat.id, send_message_response.id);
        let session_str = serde_json::to_string(&session).unwrap();
        let _: () = redis_connection
            .set_ex(key, session_str, 60 * 60)
            .await
            .unwrap();
    }

    async fn story(&self, message: &Message) {
        if let Some(duolingo) = &self.duolingo {
            let language = duolingo.languages.first().unwrap();
            let words = &duolingo.vocabulary[duolingo.vocabulary.len() - 5..]
                .iter()
                .map(|it| it.word_string.clone())
                .collect::<Vec<_>>()
                .join(",");
            let promote = format!("Please write a short story in {language} which is less than 200 words, the story should use simple words and these special words must be included: {words}. Wrap the story content in two '\"\"\"'s");
            let status_sender = self.telegram.start_sending_typing_status(message.chat.id);
            let cookie_str = env::var("EDGE_GPT_COOKIE").unwrap();
            let cookies: Vec<CookieInFile> = serde_json::from_str(&cookie_str).unwrap();
            let mut session = ChatSession::create(ConversationStyle::Creative, &cookies)
                .await
                .unwrap();
            let response = session.send_message(&promote).await.unwrap();
            let (send_message, tts_result) = self.story_respond_from_bing(message, response).await;
            status_sender.send(()).unwrap();
            self.telegram.send_message(&send_message).await;
            self.telegram.send_voice(message.chat.id, &tts_result).await;
        } else {
            let respond = simple_respond_message(
                message,
                "Please use `/duolingo_login` to login to duolingo.",
            );
            self.telegram.send_message(&respond).await;
        }
    }
}

pub fn hide_translation(
    bing_respond: &NewBingResponseMessage,
    entries: &mut Vec<MessageEntity>,
) -> String {
    let origin_text = bing_respond.text.clone();
    let re = Regex::new(r"\(([^)]+)\)").unwrap();
    for m in re.find_iter(&origin_text) {
        let utf16_start = to_utf16_offset(&origin_text, m.start());
        let utf16_size = m.as_str().encode_utf16().count();
        entries.push(MessageEntity {
            offset: utf16_start,
            length: utf16_size,
            kind: MessageEntityKind::Spoiler,
        });
    }
    re.replace_all(&origin_text, "").to_string()
}

pub fn extract_tts_part(bing_respond: &str) -> String {
    let mistake_start = bing_respond.find("Mistakes you made:");
    if let Some(mistake_start) = mistake_start {
        let mut rest = &bing_respond[mistake_start..];
        while let Some(next) = rest.find('•') {
            let line_end = rest[next..].find('\n').unwrap();
            rest = &rest[next + line_end..];
        }
        rest.trim().to_string()
    } else {
        bing_respond.trim().to_string()
    }
}

#[tokio::main]
async fn main() {
    let secret_str = env::var("SECRET").unwrap();
    let redis_url = env::var("REDIS_URL").unwrap();
    let telegram_token = env::var("TELEGRAM_TOKEN").unwrap();
    let azure_tts_subscription_key = env::var("AZURE_TTS_SUBSCRIPTION_KEY").unwrap();

    let redis_client = redis::Client::open(redis_url).unwrap();
    let secret = hex::decode(secret_str).unwrap();
    let request_encrypted = file::read("./request.json.encrypted");
    let request_str = decrypt(&hex::decode(request_encrypted).unwrap(), &secret);
    let request: Update = serde_json::from_str(&request_str).unwrap();
    if let Some(chat) = request.chat() {
        let chat_id = &chat.id;
        if chat_id.is_user() {
            let mut redis_connection = redis_client.get_async_connection().await.unwrap();
            let mut bot = if let Ok(bot_str) = redis_connection
                .get::<_, String>(format!("{chat_id}"))
                .await
            {
                if let Ok(bot) = serde_json::from_str(&bot_str) {
                    bot
                } else {
                    Bot::new(telegram_token, azure_tts_subscription_key).await
                }
            } else {
                Bot::new(telegram_token, azure_tts_subscription_key).await
            };
            if let UpdateKind::Message(message) = &request.kind {
                bot.handle(message, &mut redis_connection).await;
            }
            let bot_json = serde_json::to_string(&bot).unwrap();
            let _: () = redis_connection
                .set_ex(format!("{chat_id}"), bot_json, 60 * 60 * 24 * 30)
                .await
                .unwrap();
        } else {
            panic!("Not a user")
        }
    } else {
        panic!("Not a chat")
    };
}
