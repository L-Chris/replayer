use super::*;
use std::io::Read;

pub(super) type KeyResult = (PathBuf, Result<Option<String>, String>);
impl App {
    pub(super) fn import_qq_key(&mut self) {
        let Some(source) = self.source.clone() else {
            return;
        };
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.qq_key_rx = Some(rx);
        let language = self.settings.language;
        thread::spawn(move || {
            let result = (|| -> anyhow::Result<Option<String>> {
                let Some(path) = rfd::FileDialog::new()
                    .set_title(
                        language.text("选择当前歌曲的密钥文件", "Choose this song's key file"),
                    )
                    .add_filter("EKey", &["ekey", "txt"])
                    .pick_file()
                else {
                    return Ok(None);
                };
                let file = std::fs::File::open(path)?;
                anyhow::ensure!(
                    file.metadata()?.len() <= 16 * 1024,
                    "QQ music ekey file is too large"
                );
                let mut key = String::new();
                file.take(16 * 1024 + 1).read_to_string(&mut key)?;
                anyhow::ensure!(key.len() <= 16 * 1024, "QQ music ekey file is too large");
                Ok(Some(key.trim().to_owned()))
            })();
            let _ = tx.send((source, result.map_err(|e| format!("{e:#}"))));
        });
    }
    pub(super) fn poll_qq_key(&mut self) {
        let Some(rx) = &self.qq_key_rx else {
            return;
        };
        match rx.try_recv() {
            Ok((source, result)) => {
                self.qq_key_rx = None;
                // A late native-dialog result must never restart a different song.
                if self.source.as_ref() != Some(&source) {
                    return;
                }
                match result {
                    Ok(Some(key)) => self.load_media_with_key(
                        source.to_string_lossy().into_owned(),
                        false,
                        Some(key),
                    ),
                    Ok(None) => {}
                    Err(error) => self.error = Some(error),
                }
            }
            Err(crossbeam_channel::TryRecvError::Disconnected) => self.qq_key_rx = None,
            Err(crossbeam_channel::TryRecvError::Empty) => {}
        }
    }
}
