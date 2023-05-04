use serde::{Deserialize, Serialize};

use crate::util::new_reqwest_client;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Vocabulary {
    pub id: String,
    pub word_string: String,
    pub last_practiced_ms: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Duolingo {
    duolingo_name: String,
    duolingo_jwt: String,
    pub languages: Vec<String>,
    pub ui_language: String,
    pub vocabulary: Vec<Vocabulary>,
}

impl Duolingo {
    pub async fn new(duolingo_name: &str, duolingo_jwt: &str) -> Self {
        let (languages, ui_language) = Self::fetch_language_info(duolingo_name, duolingo_jwt).await;
        let vocabulary = Self::fetch_vocabularies(duolingo_jwt).await;
        Self {
            duolingo_name: duolingo_name.to_string(),
            duolingo_jwt: duolingo_jwt.to_string(),
            languages,
            ui_language,
            vocabulary,
        }
    }

    pub async fn from_env() -> Self {
        Self::new(
            &std::env::var("DUOLINGO_NAME").unwrap(),
            &std::env::var("DUOLINGO_JWT").unwrap(),
        )
        .await
    }

    pub async fn fetch_language_info(
        duolingo_name: &str,
        duolingo_jwt: &str,
    ) -> (Vec<String>, String) {
        let url = format!("https://www.duolingo.com/users/{duolingo_name}");
        println!("{}", url);
        let response = new_reqwest_client()
            .get(&url)
            .bearer_auth(duolingo_jwt)
            .send()
            .await
            .unwrap();
        println!("{:?}", response.text().await);
        let response = new_reqwest_client()
            .get(&url)
            .bearer_auth(duolingo_jwt)
            .send()
            .await
            .unwrap();
        let user_info: serde_json::Value = response.json().await.unwrap();
        let languages = user_info["language_data"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        let ui_language = user_info["ui_language"].as_str().unwrap().to_string();
        (languages, ui_language)
    }

    async fn fetch_vocabularies(duolingo_jwt: &str) -> Vec<Vocabulary> {
        let mut vocabulary_info: serde_json::Value = new_reqwest_client()
            .get("https://www.duolingo.com/vocabulary/overview")
            .bearer_auth(duolingo_jwt)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        serde_json::from_value(vocabulary_info["vocab_overview"].take()).unwrap()
    }
}
