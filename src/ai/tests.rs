use super::*;

fn configured() -> Settings {
    Settings {
        providers: vec![Provider {
            id: "shared".into(),
            name: "My connection".into(),
            base_url: "https://example.com/v1/".into(),
            models: vec!["audio".into(), "image-edit".into()],
            api_key: "fixture-key".into(),
        }],
    }
}

#[test]
fn business_models_resolve_independently_and_never_fall_back() {
    let mut ai = configured();
    let audio = ModelRef {
        provider: "shared".into(),
        model: "audio".into(),
    };
    let image = ModelRef {
        provider: "shared".into(),
        model: "image-edit".into(),
    };
    assert_eq!(ai.resolve(&audio).unwrap().model, "audio");
    assert_eq!(ai.resolve(&image).unwrap().model, "image-edit");
    assert!(ai.resolve(&ModelRef::default()).is_err());
    ai.providers[0].models.retain(|m| m != "audio");
    assert!(ai.resolve(&audio).is_err());
    assert!(ai.resolve(&image).is_ok());
    ai.providers[0].api_key.clear();
    assert!(ai.resolve(&image).is_err());
    ai.providers.clear();
    assert!(ai.resolve(&image).is_err());
}

#[test]
fn provider_connections_and_credentials_are_not_serialized_together() {
    let mut ai = configured();
    let json = serde_json::to_string(&ai).unwrap();
    assert!(!json.contains("fixture-key"));
    let saved: Settings = serde_json::from_str(&json).unwrap();
    assert!(saved.providers[0].api_key.is_empty());
    ai.providers[0].base_url = "https://user:fixture-key@example.com/v1".into();
    let error = ai.validate().err().unwrap().to_string();
    assert!(!error.contains("fixture-key"));
}

#[test]
fn fetch_models_uses_the_selected_connection_and_hides_upstream_secrets() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        for (status, body) in [
            (
                200,
                r#"{"data":[{"id":"z/model"},{"id":"audio"},{"id":"audio"}]}"#,
            ),
            (401, "fixture-key"),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            loop {
                let mut buffer = [0; 1024];
                let len = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..len]);
                if len == 0 || request.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let request = String::from_utf8(request).unwrap().to_lowercase();
            assert!(request.starts_with("get /v1/models "));
            assert!(request.contains("authorization: bearer fixture-key"));
            write!(stream, "HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut provider = configured().providers.remove(0);
        provider.base_url = format!("http://{address}/v1");
        assert_eq!(
            fetch_models(&provider).await.unwrap(),
            vec!["audio", "z/model"]
        );
        let error = fetch_models(&provider).await.unwrap_err().to_string();
        assert!(error.contains("401"));
        assert!(!error.contains("fixture-key"));
        assert!(!error.contains(&provider.base_url));
    });
    worker.join().unwrap();
}

#[test]
fn client_ignores_service_environment_and_dotenv_files() {
    const CHILD: &str = "PORDO_TEST_ENV_ISOLATION_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let settings = crate::settings::Settings::load();
        assert!(settings.ai.providers.is_empty());
        assert!(settings.music.key().unwrap().is_empty());
        assert!(crate::subtitles::LlmConfig::load().is_err());
        assert!(
            crate::comics::translation::Config::load_with(
                &settings.comics,
                &settings.ai,
                settings.language
            )
            .is_err()
        );
        return;
    }
    let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    let directory = target.join(format!("config-isolation-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join(".env");
    std::fs::write(&file, "LLM_API_KEY=dotenv-fixture\nLLM_BASE_URL=http://127.0.0.1:9/v1\nLLM_MODEL=unwanted\nIMAGE_MODEL=unwanted\nCHKSZ_API_KEY=dotenv-fixture\n").unwrap();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "ai::tests::client_ignores_service_environment_and_dotenv_files",
            "--nocapture",
        ])
        .current_dir(&directory)
        .env(CHILD, "1")
        .env("PORDO_SETTINGS_FILE", directory.join("settings.json"))
        .env("PORDO_ENV_FILE", &file)
        .env("LLM_API_KEY", "environment-fixture")
        .env("OPENAI_API_KEY", "environment-fixture")
        .env("LLM_BASE_URL", "http://127.0.0.1:9/v1")
        .env("LLM_MODEL", "unwanted")
        .env("IMAGE_API_KEY", "environment-fixture")
        .env("IMAGE_MODEL", "unwanted")
        .env("CHKSZ_API_KEY", "environment-fixture")
        .output()
        .unwrap();
    assert!(directory.starts_with(target));
    std::fs::remove_dir_all(directory).unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
