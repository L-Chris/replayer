use super::*;

/// Provider identity is persisted in Book, independently of upstream site IDs.
pub struct Sources {
    providers: Vec<(Site, Arc<dyn ComicSource>)>,
}
impl Sources {
    pub fn connect() -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            providers: vec![
                (
                    Site {
                        id: "mangayun".into(),
                        name: "MangaYun".into(),
                    },
                    MangaYun::connect()?,
                ),
                (
                    Site {
                        id: "komiic".into(),
                        name: "Komiic".into(),
                    },
                    Arc::new(Komiic::at("https://komiic.com")?),
                ),
            ],
        }))
    }
    pub fn providers(&self) -> Vec<(Site, Arc<dyn ComicSource>)> {
        self.providers.clone()
    }
    pub fn qualify(provider: &str, groups: &mut [SearchGroup]) {
        for group in groups {
            group.site.id = format!("{provider}:{}", group.site.id);
        }
    }
    fn provider(&self, book: &Book) -> Result<&dyn ComicSource> {
        self.providers
            .iter()
            .find(|(site, _)| {
                site.id == book.provider || (book.provider.is_empty() && site.id == "mangayun")
            })
            .map(|(_, source)| source.as_ref())
            .ok_or_else(|| anyhow!("不支持的漫画源 / Unsupported comic provider"))
    }
}
impl ComicSource for Sources {
    fn image_policy(&self, book: &Book) -> ImagePolicy {
        self.provider(book)
            .map(|source| source.image_policy(book))
            .unwrap_or_default()
    }
    fn sites(&self) -> BoxFuture<'_, Result<Vec<Site>>> {
        Box::pin(async move {
            let results = futures_util::future::join_all(self.providers.iter().map(
                |(site, source)| async move {
                    let mut sites = source.sites().await?;
                    for child in &mut sites {
                        child.id = format!("{}:{}", site.id, child.id);
                    }
                    Ok::<_, anyhow::Error>(sites)
                },
            ))
            .await;
            Ok(results
                .into_iter()
                .filter_map(Result::ok)
                .flatten()
                .collect())
        })
    }
    fn search<'a>(&'a self, keyword: &'a str) -> BoxFuture<'a, Result<Vec<SearchGroup>>> {
        Box::pin(async move {
            let results = futures_util::future::join_all(self.providers.iter().map(
                |(site, source)| async move {
                    let mut groups = source.search(keyword).await.unwrap_or_else(|error| {
                        vec![SearchGroup {
                            site: site.clone(),
                            books: vec![],
                            error: Some(error.to_string()),
                        }]
                    });
                    Self::qualify(&site.id, &mut groups);
                    groups
                },
            ))
            .await;
            Ok(results.into_iter().flatten().collect())
        })
    }
    fn detail<'a>(&'a self, book: &'a Book) -> BoxFuture<'a, Result<Detail>> {
        Box::pin(async move { self.provider(book)?.detail(book).await })
    }
    fn pages<'a>(
        &'a self,
        book: &'a Book,
        chapter: &'a Chapter,
        refresh: bool,
    ) -> BoxFuture<'a, Result<Vec<String>>> {
        Box::pin(async move { self.provider(book)?.pages(book, chapter, refresh).await })
    }
    fn image_url(&self, _site: &str, _url: &str) -> Result<reqwest::Url> {
        anyhow::bail!("图片请求缺少来源 / Missing comic provider")
    }
    fn book_image_request(&self, book: &Book, url: &str) -> Result<ImageRequest> {
        self.provider(book)?.image_request(&book.site, url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_identity_routes_images_and_retains_existing_book_keys() {
        let sources = Sources::connect().unwrap();
        let book = Book {
            provider: "mangayun".into(),
            site: "dm5".into(),
            url: "https://www.dm5.com/book".into(),
            ..Default::default()
        };
        let key = book.key();
        assert_eq!(
            sources
                .book_image_request(&book, "https://img.example/page.jpg")
                .unwrap()
                .url
                .path(),
            "/api/image"
        );
        assert_eq!(book.key(), key);
        let komiic = Book {
            provider: "komiic".into(),
            site: "dm5".into(),
            ..book.clone()
        };
        assert!(
            sources
                .book_image_request(&komiic, "https://img.example/page.jpg")
                .is_err()
        );
        assert_eq!(
            sources
                .book_image_request(&komiic, "https://public.komiic.com/cover.jpg")
                .unwrap()
                .url
                .host_str(),
            Some("public.komiic.com")
        );
        assert_ne!(book.key(), komiic.key());
        assert!(
            sources
                .book_image_request(
                    &Book {
                        provider: "unknown".into(),
                        ..book
                    },
                    "https://public.komiic.com/cover.jpg"
                )
                .is_err()
        );
    }
}
