mod azure_tts;
mod bing_dictionary;
mod duolingo;
mod telegram;
mod util;
mod edge_gpt;
use bing_dictionary::Word;
use ezio::prelude::*;
use rand::prelude::*;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::env;
use telegram::simple_respond_message;
use teloxide::types::{Message, Update, UpdateKind};
use util::decrypt;
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    Start,
    DuolingoLogin,
    RandomWord,
    Chat,
    Passage,
    Help,
}

impl TryFrom<&str> for CommandKind {
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "start" => Ok(Self::Start),
            "duolingo_login" => Ok(Self::DuolingoLogin),
            "random_word" => Ok(Self::RandomWord),
            "chat" => Ok(Self::Chat),
            "passage" => Ok(Self::Passage),
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

    pub async fn handle(&mut self, message: &Message) {
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
                            println!("{params_str}");
                            let mut params = params_str.trim().split(' ');
                            let name = params.next().unwrap();
                            let jwt = params.next().unwrap();
                            let duolingo = duolingo::Duolingo::new(name, jwt).await;
                            self.duolingo = Some(duolingo);
                        }
                        CommandKind::RandomWord => {
                            if let Some(duolingo) = &self.duolingo {
                                let vocabulary = {
                                    let mut rng = thread_rng();
                                    duolingo.vocabulary.choose(&mut rng).unwrap()
                                };
                                let language = duolingo.languages.first().unwrap();
                                let status_sender =
                                    self.telegram.start_sending_typing_status(message.chat.id);
                                let word = Word::from_vocabulary(
                                    vocabulary,
                                    duolingo.ui_language.as_ref(),
                                    language,
                                )
                                .await;
                                let (text, word, sentence) = word
                                    .to_telegram_message(&self.azure_tts, language, message.chat.id)
                                    .await;
                                status_sender.send(()).unwrap();
                                self.telegram.send_message(&text).await;
                                self.telegram.send_voice(&word).await;
                                self.telegram.send_voice(&sentence).await;
                            } else {
                                let respond = simple_respond_message(
                                    message,
                                    "Please use `/duolingo_login` to login to duolingo.",
                                );
                                self.telegram.send_message(&respond).await;
                            }
                        }
                        _ => {
                            unimplemented!()
                        }
                    }
                }
            }
        }
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
                bot.handle(message).await;
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
