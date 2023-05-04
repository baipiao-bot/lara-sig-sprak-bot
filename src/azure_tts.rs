use crate::util::new_reqwest_client;
use bytes::Bytes;
use core::fmt;
use serde::{Deserialize, Serialize};

const TTS_URL: &str = "https://northeurope.tts.speech.microsoft.com/cognitiveservices/v1";
const VOICE_LIST_URL: &str =
    "https://northeurope.tts.speech.microsoft.com/cognitiveservices/voices/list";

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub enum Gender {
    Male,
    Female,
    #[serde(other)]
    Other,
}

impl fmt::Display for Gender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Gender::Male => write!(f, "Male"),
            Gender::Female => write!(f, "Female"),
            Gender::Other => unimplemented!(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Voice {
    name: String,
    short_name: String,
    pub gender: Gender,
    pub locale: String,
    #[serde(default)]
    style_list: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AzureTTS {
    subscription_key: String,
    pub voices: Vec<Voice>,
}

impl AzureTTS {
    pub async fn new(subscription_key: impl ToString) -> Self {
        let voices = new_reqwest_client()
            .get(VOICE_LIST_URL)
            .header("Ocp-Apim-Subscription-Key", subscription_key.to_string())
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        Self {
            subscription_key: subscription_key.to_string(),
            voices,
        }
    }

    pub async fn from_env() -> Self {
        Self::new(std::env::var("AZURE_TTS_SUBSCRIPTION_KEY").unwrap()).await
    }

    pub async fn tts_simple(&self, text: &str, voice: &Voice) -> Bytes {
        self.tts(&[(text, voice)]).await
    }

    pub async fn tts(&self, content: &[(&str, &Voice)]) -> Bytes {
        let locale = content[0].1.locale.clone();
        let mut tts_ssml = format!("<speak version='1.0' xml:lang='{locale}'>");
        for (
            text,
            Voice {
                short_name, gender, ..
            },
        ) in content
        {
            tts_ssml += &format!(
                "<voice xml:lang='{locale}' xml:gender='{gender}' name='{short_name}'>{text}</voice>"
            )
        }
        tts_ssml += "</speak>";
        let response = new_reqwest_client()
            .post(TTS_URL)
            .header("Ocp-Apim-Subscription-Key", &self.subscription_key)
            .header("Content-Type", "application/ssml+xml")
            .header("X-Microsoft-OutputFormat", "ogg-16khz-16bit-mono-opus")
            .body(tts_ssml)
            .send()
            .await
            .unwrap();
        response.bytes().await.unwrap()
    }
}
