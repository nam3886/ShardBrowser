//! Frame owners (the Cloudflare `<iframe>` behind a closed shadow root), cookies
//! and site storage.

use chromiumoxide::cdp::browser_protocol::dom::GetFrameOwnerParams;
use chromiumoxide::cdp::browser_protocol::network::DeleteCookiesParams;
use chromiumoxide::cdp::browser_protocol::page::{FrameId, FrameTree, GetFrameTreeParams};
use chromiumoxide::cdp::browser_protocol::storage::{ClearDataForOriginParams, GetCookiesParams};
use chromiumoxide::cdp::browser_protocol::target::GetTargetsParams;

use super::locator::Locator;
use super::{cdp, Page, WebResult};

/// Name and domain only: cookie values never leave this module.
#[derive(Clone, Debug, PartialEq)]
pub struct Cookie {
    pub name: String,
    pub domain: String,
    path: String,
}

/// `domain` (a cookie's, maybe with a leading dot) is `suffix` or a subdomain of it.
fn under(domain: &str, suffix: &str) -> bool {
    let d = domain.trim_start_matches('.').to_ascii_lowercase();
    let s = suffix.trim_start_matches('.').to_ascii_lowercase();
    d == s || d.ends_with(&format!(".{s}"))
}

fn host(url: &str) -> Option<String> {
    url::Url::parse(url).ok()?.host_str().map(str::to_string)
}

fn walk<'a>(t: &'a FrameTree, out: &mut Vec<&'a FrameTree>) {
    out.push(t);
    for c in t.child_frames.iter().flatten() {
        walk(c, out);
    }
}

impl Page {
    /// The `<iframe>` elements of the frames on `host` (compared exactly, never
    /// another site's frame). Works for out-of-process frames and closed shadow
    /// roots, which selectors cannot reach (Python `cf.cf_frames`).
    pub async fn frame_owners(&self, host_name: &str) -> WebResult<Vec<Locator>> {
        self.watch
            .act(async {
                // In-process frames are in this page's frame tree; an out-of-process
                // one leaves it and shows as an `iframe` target whose id is its frame id.
                let tree = self
                    .inner
                    .execute(GetFrameTreeParams::default())
                    .await
                    .map_err(cdp)?;
                let mut all = vec![];
                walk(&tree.result.frame_tree, &mut all);
                let mut frames: Vec<(FrameId, String)> = all
                    .into_iter()
                    .skip(1)
                    .map(|t| (t.frame.id.clone(), t.frame.url.clone()))
                    .collect();
                let targets = self
                    .inner
                    .execute(GetTargetsParams::default())
                    .await
                    .map_err(cdp)?;
                frames.extend(
                    targets
                        .result
                        .target_infos
                        .iter()
                        .filter(|t| t.r#type == "iframe")
                        .map(|t| (FrameId::new(t.target_id.inner().clone()), t.url.clone())),
                );
                let mut out = vec![];
                for (id, url) in frames {
                    if host(&url).as_deref() != Some(host_name) {
                        continue;
                    }
                    // Another page's frame, or one detached meanwhile, has no owner here.
                    if let Ok(o) = self.inner.execute(GetFrameOwnerParams::new(id)).await {
                        out.push(self.node(o.result.backend_node_id));
                    }
                }
                Ok(out)
            })
            .await
    }

    /// The browser's cookies whose domain is `domain_suffix` or under it.
    pub async fn cookies(&self, domain_suffix: &str) -> WebResult<Vec<Cookie>> {
        self.watch
            .act(async {
                let r = self
                    .inner
                    .execute(GetCookiesParams::default())
                    .await
                    .map_err(cdp)?;
                Ok(r.result
                    .cookies
                    .iter()
                    .filter(|c| under(&c.domain, domain_suffix))
                    .map(|c| Cookie {
                        name: c.name.clone(),
                        domain: c.domain.clone(),
                        path: c.path.clone(),
                    })
                    .collect())
            })
            .await
    }

    pub async fn clear_cookies(&self, domain_suffix: &str) -> WebResult<()> {
        for c in self.cookies(domain_suffix).await? {
            let p = DeleteCookiesParams {
                name: c.name,
                domain: Some(c.domain),
                path: Some(c.path),
                url: None,
                partition_key: None,
            };
            self.watch
                .act(async { self.inner.execute(p).await.map(|_| ()).map_err(cdp) })
                .await?;
        }
        Ok(())
    }

    /// Local/session storage, IndexedDB and Cache Storage of `origin` (`https://x.y`).
    pub async fn clear_storage(&self, origin: &str) -> WebResult<()> {
        let p = ClearDataForOriginParams::new(
            origin,
            "local_storage,session_storage,indexeddb,cache_storage",
        );
        self.watch
            .act(async { self.inner.execute(p).await.map(|_| ()).map_err(cdp) })
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::super::testkit::{FixtureServer, TestBrowser};
    use super::super::World;
    use super::*;

    /// Python `cf.TOKEN_JS`: the widget's own response field holds a token.
    const TOKEN_JS: &str = "(f) => { const r = f.getRootNode(); let n = r instanceof ShadowRoot ? r.host : f;
        for (let i = 0; n && i < 4; i++, n = n.parentElement) {
            const t = n.querySelector(\"input[name='cf-turnstile-response']\"); if (t) return !!t.value; }
        return false; }";

    #[test]
    fn domain_suffix_is_on_a_label_boundary() {
        assert!(under(".x.ai", "x.ai"));
        assert!(under("auth.x.ai", "x.ai"));
        assert!(under("X.AI", "x.ai"));
        assert!(!under("max.ai", "x.ai"));
    }

    #[tokio::test]
    async fn frame_owner_of_cross_site_iframe() {
        let Some(b) = TestBrowser::launch().await else {
            return;
        };
        let site = FixtureServer::start().await;
        let page = b.page().await;
        let url = format!("{}/frames.html?other={}", site.site, site.other);
        page.goto(&url).await.unwrap();
        // The frame only shows in the tree once it starts loading.
        let mut owners = vec![];
        for _ in 0..50 {
            owners = page.frame_owners("localhost").await.unwrap();
            if !owners.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert_eq!(owners.len(), 1);
        // The same-site frame (the main frame itself is never an owner).
        assert_eq!(page.frame_owners("127.0.0.1").await.unwrap().len(), 1);
        // Behind a closed shadow root: css sees only the same-site one.
        assert_eq!(page.css("iframe").count().await.unwrap(), 1);
        let f = &owners[0];
        let r = f.bounding_box().await.unwrap().unwrap();
        assert!(r.width > 0.0 && r.height > 0.0);
        assert!(!f.evaluate::<bool>(TOKEN_JS).await.unwrap());
        // The widget posts its token; the page puts it in the response field.
        page.eval::<()>("() => { window.askToken(); }", World::Main)
            .await
            .unwrap();
        let mut token = false;
        for _ in 0..50 {
            token = f.evaluate::<bool>(TOKEN_JS).await.unwrap();
            if token {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(token);
    }

    #[tokio::test]
    async fn cookies_by_domain_and_clear() {
        let Some(b) = TestBrowser::launch().await else {
            return;
        };
        let site = FixtureServer::start().await;
        let page = b.page().await;
        page.goto(&format!("{}/basic.html", site.site))
            .await
            .unwrap();
        page.eval::<()>(
            "() => { document.cookie = 'a=1; path=/'; document.cookie = 'b=2; path=/'; }",
            World::Main,
        )
        .await
        .unwrap();
        let mut names: Vec<String> = page
            .cookies("127.0.0.1")
            .await
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        names.sort();
        assert_eq!(names, ["a", "b"]);
        assert!(page.cookies("localhost").await.unwrap().is_empty());
        page.clear_cookies("127.0.0.1").await.unwrap();
        assert!(page.cookies("127.0.0.1").await.unwrap().is_empty());
        let left: String = page
            .eval("() => document.cookie", World::Main)
            .await
            .unwrap();
        assert_eq!(left, "");
    }

    #[tokio::test]
    async fn clear_storage_empties_local_storage() {
        let Some(b) = TestBrowser::launch().await else {
            return;
        };
        let site = FixtureServer::start().await;
        let page = b.page().await;
        page.goto(&format!("{}/basic.html", site.site))
            .await
            .unwrap();
        let set = "() => { localStorage.setItem('k', 'v'); sessionStorage.setItem('k', 'v'); }";
        page.eval::<()>(set, World::Main).await.unwrap();
        page.clear_storage(&site.site).await.unwrap();
        let get = "() => [localStorage.length, sessionStorage.length]";
        let lens: (u32, u32) = page.eval(get, World::Main).await.unwrap();
        assert_eq!(lens.0, 0);
        assert_eq!(lens.1, 0);
    }
}
