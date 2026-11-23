//! Locators with Playwright's matching rules (as the Python flows rely on them).
//!
//! A locator is a recipe, resolved afresh on every call (pages re-render):
//! css through `querySelectorAll`, role through the browser's own accessibility
//! tree (`Accessibility.queryAXTree`, nothing injected), text in the isolated
//! world. Element handles live in one object group per call, released after it.

use std::sync::atomic::{AtomicU64, Ordering};

use chromiumoxide::cdp::browser_protocol::dom::{
    BackendNodeId, DescribeNodeParams, ResolveNodeParams,
};
use chromiumoxide::cdp::js_protocol::runtime::{
    CallArgument, CallFunctionOnParams, GetPropertiesParams, ReleaseObjectGroupParams,
    RemoteObjectId,
};
use regex::Regex;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::{cdp, Page, Rect, WebError, WebResult, World};

/// `Accessibility.queryAXTree`, answered as plain JSON: the generated 0.7 types
/// cannot read the ignored reasons today's Chrome sends (`ariaHiddenSubtree`),
/// so any page with an aria-hidden node failed the whole query. (Copied shape of
/// `shardx::motion::RawCommand`, whose fields are private.)
#[derive(serde::Serialize)]
struct QueryAxTree {
    #[serde(rename = "objectId")]
    object_id: RemoteObjectId,
    role: String,
}

impl chromiumoxide::Method for QueryAxTree {
    fn identifier(&self) -> chromiumoxide::types::MethodId {
        "Accessibility.queryAXTree".into()
    }
}

impl chromiumoxide::Command for QueryAxTree {
    type Response = Value;
}

/// An accessible name or a text to match. A plain string is a case-insensitive
/// substring of the whitespace-normalised text; a regex is matched as given.
#[derive(Clone, Debug)]
pub enum Name {
    Sub(String),
    Re(Regex),
}

impl From<&str> for Name {
    fn from(s: &str) -> Self {
        Name::Sub(s.to_string())
    }
}

impl From<Regex> for Name {
    fn from(r: Regex) -> Self {
        Name::Re(r)
    }
}

/// A role locator's optional name: `"text"`, a `Regex`, a `Name` or `None`.
pub trait RoleName {
    fn into_name(self) -> Option<Name>;
}

impl RoleName for Option<Name> {
    fn into_name(self) -> Option<Name> {
        self
    }
}

impl<T: Into<Name>> RoleName for T {
    fn into_name(self) -> Option<Name> {
        Some(self.into())
    }
}

pub(crate) fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Name {
    pub fn matches(&self, text: &str) -> bool {
        let text = normalize(text);
        match self {
            Name::Sub(s) => text.to_lowercase().contains(&normalize(s).to_lowercase()),
            Name::Re(r) => r.is_match(&text),
        }
    }

    /// The same test for the page's JS: `[kind, source, flags]`.
    fn js(&self) -> [Value; 3] {
        match self {
            Name::Sub(s) => [json!("sub"), json!(normalize(s).to_lowercase()), json!("")],
            Name::Re(r) => {
                let (src, flags) = js_regex(r.as_str());
                [json!("re"), json!(src), json!(flags)]
            }
        }
    }
}

/// A Rust pattern as a JS `RegExp` source + flags: a leading inline flag group
/// (`(?i)`, `(?is)`) becomes flags.
// ponytail: only the leading flag group is translated; the rest of the syntax is
// shared by the simple patterns the flows use. A text regex with Rust-only syntax
// (named groups `(?P<..>)`, `\A`/`\z`) would need a real translation.
fn js_regex(src: &str) -> (String, String) {
    if let Some(rest) = src.strip_prefix("(?") {
        if let Some(end) = rest.find(')') {
            let flags = &rest[..end];
            if !flags.is_empty() && flags.chars().all(|c| "ims".contains(c)) {
                return (rest[end + 1..].to_string(), flags.to_string());
            }
        }
    }
    (src.to_string(), String::new())
}

#[derive(Clone, Debug)]
pub(crate) enum Step {
    Css(String),
    Role(String, Option<Name>),
    Text(Name),
    FilterText(Name),
    /// Negative counts from the end (-1 = last).
    Nth(i64),
    Node(BackendNodeId),
}

#[derive(Clone)]
pub struct Locator {
    pub(crate) page: Page,
    steps: Vec<Step>,
}

impl Page {
    pub fn css(&self, sel: &str) -> Locator {
        Locator::start(self, Step::Css(sel.to_string()))
    }

    pub fn role(&self, role: &str, name: impl RoleName) -> Locator {
        Locator::start(self, Step::Role(role.to_string(), name.into_name()))
    }

    pub fn text(&self, name: impl Into<Name>) -> Locator {
        Locator::start(self, Step::Text(name.into()))
    }

    /// A locator pinned to one node.
    pub(crate) fn node(&self, id: BackendNodeId) -> Locator {
        Locator::start(self, Step::Node(id))
    }
}

/// Scripts run on an element (`this`) in the isolated world.
const NORM_JS: &str = "const norm = (s) => (s || '').replace(/\\s+/g, ' ').trim();";
const CSS_JS: &str = "function (sel) { return Array.from(this.querySelectorAll(sel)); }";
const VISIBLE_JS: &str = "function () { const r = this.getBoundingClientRect();
    return r.width > 0 && r.height > 0 && this.checkVisibility({ visibilityProperty: true, opacityProperty: false }); }";
const DISABLED_JS: &str =
    "function () { return this.matches(':disabled') || !!this.closest('[aria-disabled=true]'); }";
const BOX_JS: &str = "function () { const r = this.getBoundingClientRect();
    return { x: r.x, y: r.y, width: r.width, height: r.height }; }";

fn text_js() -> String {
    // The smallest elements whose text matches: an element whose child matches is
    // not itself a match. Disjoint subtrees, so the order is document order.
    format!(
        "function (kind, needle, flags) {{ {NORM_JS}
        const re = kind === 're' ? new RegExp(needle, flags) : null;
        const test = (t) => re ? re.test(t) : t.toLowerCase().includes(needle);
        const skip = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE', 'HEAD']);
        const out = [];
        const walk = (el) => {{
            if (skip.has(el.tagName)) return false;
            let child = false;
            for (const c of el.children) if (walk(c)) child = true;
            if (child) return true;
            if (test(norm(el.textContent))) {{ out.push(el); return true; }}
            return false;
        }};
        walk(this.nodeType === 9 ? this.documentElement : this);
        return out; }}"
    )
}

fn text_of_js() -> String {
    format!("function () {{ {NORM_JS} return norm(this.textContent); }}")
}

const UNIQUE_JS: &str = "function (...els) { return [...new Set(els)].sort((a, b) => \
    a === b ? 0 : a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1); }";

static GROUP: AtomicU64 = AtomicU64::new(0);

/// One call's element handles, in the isolated world.
pub(crate) struct Handles<'a> {
    pub(crate) page: &'a Page,
    pub(crate) group: String,
    ctx: chromiumoxide::cdp::js_protocol::runtime::ExecutionContextId,
}

impl<'a> Handles<'a> {
    pub(crate) async fn new(page: &'a Page) -> WebResult<Handles<'a>> {
        Ok(Handles {
            page,
            group: format!("creator-{}", GROUP.fetch_add(1, Ordering::Relaxed)),
            ctx: page.context(World::Isolated).await?,
        })
    }

    pub(crate) async fn release(self) {
        let _ = self
            .page
            .inner
            .execute(ReleaseObjectGroupParams::new(self.group.clone()))
            .await;
    }

    async fn document(&self) -> WebResult<RemoteObjectId> {
        let mut call = CallFunctionOnParams::new("function () { return document; }");
        call.execution_context_id = Some(self.ctx);
        call.object_group = Some(self.group.clone());
        let r = self.page.inner.execute(call).await.map_err(cdp)?;
        r.result.result.object_id.clone().ok_or(WebError::NotFound)
    }

    pub(crate) async fn resolve(&self, id: BackendNodeId) -> WebResult<RemoteObjectId> {
        let p = ResolveNodeParams {
            backend_node_id: Some(id),
            object_group: Some(self.group.clone()),
            execution_context_id: Some(self.ctx),
            ..Default::default()
        };
        let r = self.page.inner.execute(p).await.map_err(cdp)?;
        r.result.object.object_id.clone().ok_or(WebError::NotFound)
    }

    pub(crate) async fn backend_id(&self, obj: &RemoteObjectId) -> WebResult<BackendNodeId> {
        let p = DescribeNodeParams {
            object_id: Some(obj.clone()),
            ..Default::default()
        };
        let r = self.page.inner.execute(p).await.map_err(cdp)?;
        Ok(r.result.node.backend_node_id)
    }

    /// `f` called on `obj` with JSON `args`; by value, or as an array of handles.
    async fn call(
        &self,
        obj: &RemoteObjectId,
        f: &str,
        args: &[Value],
        by_value: bool,
    ) -> WebResult<chromiumoxide::cdp::js_protocol::runtime::RemoteObject> {
        let mut call = CallFunctionOnParams::new(f);
        call.object_id = Some(obj.clone());
        call.arguments = Some(
            args.iter()
                .map(|v| CallArgument {
                    value: Some(v.clone()),
                    ..Default::default()
                })
                .collect(),
        );
        call.return_by_value = Some(by_value);
        call.await_promise = Some(true);
        let r = self.page.inner.execute(call).await.map_err(cdp)?;
        if let Some(ex) = &r.result.exception_details {
            let text = ex
                .exception
                .as_ref()
                .and_then(|e| e.description.clone())
                .unwrap_or_else(|| ex.text.clone());
            return Err(WebError::Cdp(text.lines().next().unwrap_or("").to_string()));
        }
        Ok(r.result.result.clone())
    }

    pub(crate) async fn value<T: DeserializeOwned>(
        &self,
        obj: &RemoteObjectId,
        f: &str,
        args: &[Value],
    ) -> WebResult<T> {
        let v = self
            .call(obj, f, args, true)
            .await?
            .value
            .unwrap_or_default();
        serde_json::from_value(v).map_err(cdp)
    }

    async fn elements(
        &self,
        obj: &RemoteObjectId,
        f: &str,
        args: &[Value],
    ) -> WebResult<Vec<RemoteObjectId>> {
        let Some(arr) = self.call(obj, f, args, false).await?.object_id else {
            return Ok(vec![]);
        };
        self.items(arr).await
    }

    /// The elements of a JS array handle, in index order.
    async fn items(&self, arr: RemoteObjectId) -> WebResult<Vec<RemoteObjectId>> {
        let mut p = GetPropertiesParams::new(arr);
        p.own_properties = Some(true);
        let r = self.page.inner.execute(p).await.map_err(cdp)?;
        let mut out: Vec<(usize, RemoteObjectId)> = r
            .result
            .result
            .iter()
            .filter_map(|d| {
                let i = d.name.parse::<usize>().ok()?;
                Some((i, d.value.as_ref()?.object_id.clone()?))
            })
            .collect();
        out.sort_by_key(|(i, _)| *i);
        Ok(out.into_iter().map(|(_, o)| o).collect())
    }

    /// Each element once, in document order: scopes that nest (a dialog inside a
    /// dialog) find the same element twice (Playwright dedupes too).
    async fn unique(&self, nodes: Vec<RemoteObjectId>) -> WebResult<Vec<RemoteObjectId>> {
        if nodes.len() < 2 {
            return Ok(nodes);
        }
        let mut call = CallFunctionOnParams::new(UNIQUE_JS);
        call.object_id = Some(nodes[0].clone());
        call.arguments = Some(
            nodes
                .iter()
                .map(|o| CallArgument {
                    object_id: Some(o.clone()),
                    ..Default::default()
                })
                .collect(),
        );
        call.object_group = Some(self.group.clone());
        let r = self.page.inner.execute(call).await.map_err(cdp)?;
        match r.result.result.object_id.clone() {
            Some(arr) => self.items(arr).await,
            None => Ok(nodes),
        }
    }

    async fn by_role(
        &self,
        scope: &RemoteObjectId,
        role: &str,
        name: &Option<Name>,
    ) -> WebResult<Vec<RemoteObjectId>> {
        let p = QueryAxTree {
            object_id: scope.clone(),
            role: role.to_string(),
        };
        let r = self.page.inner.execute(p).await.map_err(cdp)?;
        let mut out = vec![];
        for n in r.result["nodes"].as_array().into_iter().flatten() {
            // Out of the accessibility tree (aria-hidden, visibility:hidden, inert):
            // Playwright's role engine skips these, e.g. the page behind a modal.
            if n["ignored"].as_bool() == Some(true) {
                continue;
            }
            let accessible = n["name"]["value"].as_str().unwrap_or("");
            if name.as_ref().is_some_and(|nm| !nm.matches(accessible)) {
                continue;
            }
            if let Some(id) = n["backendDOMNodeId"].as_i64() {
                out.push(self.resolve(BackendNodeId::new(id)).await?);
            }
        }
        Ok(out)
    }

    /// The elements `steps` select, in document order.
    async fn select(&self, steps: &[Step]) -> WebResult<Vec<RemoteObjectId>> {
        let mut nodes = vec![self.document().await?];
        for step in steps {
            let scopes = nodes.len();
            let mut next = vec![];
            match step {
                Step::Nth(i) => {
                    let at = if *i < 0 { nodes.len() as i64 + i } else { *i };
                    next.extend(
                        usize::try_from(at)
                            .ok()
                            .and_then(|at| nodes.get(at).cloned()),
                    );
                }
                Step::Node(id) => next.push(self.resolve(*id).await?),
                Step::FilterText(name) => {
                    let js = text_of_js();
                    for n in nodes {
                        let text: String = self.value(&n, &js, &[]).await?;
                        if name.matches(&text) {
                            next.push(n);
                        }
                    }
                }
                Step::Css(sel) => {
                    for n in &nodes {
                        next.extend(self.elements(n, CSS_JS, &[json!(sel)]).await?);
                    }
                }
                Step::Role(role, name) => {
                    for n in &nodes {
                        next.extend(self.by_role(n, role, name).await?);
                    }
                }
                Step::Text(name) => {
                    let (js, args) = (text_js(), name.js());
                    for n in &nodes {
                        next.extend(self.elements(n, &js, &args).await?);
                    }
                }
            }
            nodes = match step {
                Step::Css(_) | Step::Role(..) | Step::Text(_) if scopes > 1 => {
                    self.unique(next).await?
                }
                _ => next,
            };
        }
        Ok(nodes)
    }
}

impl Locator {
    fn start(page: &Page, step: Step) -> Locator {
        Locator {
            page: page.clone(),
            steps: vec![step],
        }
    }

    pub(crate) fn steps(&self) -> Vec<Step> {
        self.steps.clone()
    }

    pub(crate) fn with_steps(page: &Page, steps: Vec<Step>) -> Locator {
        Locator {
            page: page.clone(),
            steps,
        }
    }

    fn then(&self, step: Step) -> Locator {
        let mut steps = self.steps.clone();
        steps.push(step);
        Locator {
            page: self.page.clone(),
            steps,
        }
    }

    /// Scoped: matches inside each match of `self`.
    pub fn css(&self, sel: &str) -> Locator {
        self.then(Step::Css(sel.to_string()))
    }

    pub fn role(&self, role: &str, name: impl RoleName) -> Locator {
        self.then(Step::Role(role.to_string(), name.into_name()))
    }

    pub fn text(&self, name: impl Into<Name>) -> Locator {
        self.then(Step::Text(name.into()))
    }

    /// Only the matches whose text matches `name` (Playwright `filter(has_text=)`).
    pub fn filter_text(&self, name: impl Into<Name>) -> Locator {
        self.then(Step::FilterText(name.into()))
    }

    pub fn first(&self) -> Locator {
        self.nth(0)
    }

    pub fn last(&self) -> Locator {
        self.then(Step::Nth(-1))
    }

    pub fn nth(&self, i: usize) -> Locator {
        self.then(Step::Nth(i as i64))
    }

    /// Run `f` with the handles of this locator's matches (one object group).
    pub(crate) async fn with<T>(
        &self,
        mut f: impl AsyncFnMut(&Handles<'_>, Vec<RemoteObjectId>) -> WebResult<T>,
    ) -> WebResult<T> {
        // A navigation between resolving and using the handles makes them stale:
        // the whole call runs again on the new document.
        let work = super::retry_stale(async || {
            let h = Handles::new(&self.page).await?;
            let r = match h.select(&self.steps).await {
                Ok(nodes) => f(&h, nodes).await,
                Err(e) => Err(e),
            };
            h.release().await;
            r
        });
        self.page.watch.act(work).await
    }

    /// Hidden matches count too.
    pub async fn count(&self) -> WebResult<usize> {
        self.with(async |_, nodes| Ok(nodes.len())).await
    }

    /// The first visible match (Python `_first_visible`: sites render desktop and
    /// mobile copies, and the first one can be hidden), as `nth(i)` of this
    /// locator: re-resolved on every use, like any locator.
    pub async fn first_visible(&self) -> WebResult<Option<Locator>> {
        let found = self
            .with(async |h, nodes| {
                for (i, n) in nodes.iter().enumerate() {
                    if h.value::<bool>(n, VISIBLE_JS, &[]).await? {
                        return Ok(Some(i));
                    }
                }
                Ok(None)
            })
            .await?;
        Ok(found.map(|i| self.nth(i)))
    }

    /// The first match's own value of `f` (`function () { … this … }`), or NotFound.
    async fn on_first<T: DeserializeOwned>(&self, f: &str) -> WebResult<T> {
        self.with(async |h, nodes| match nodes.first() {
            Some(n) => h.value(n, f, &[]).await,
            None => Err(WebError::NotFound),
        })
        .await
    }

    /// The first match is visible (false when there is none).
    pub async fn is_visible(&self) -> bool {
        self.on_first::<bool>(VISIBLE_JS).await.unwrap_or(false)
    }

    /// The first match is disabled: `disabled` (also through a fieldset) or `aria-disabled`.
    pub async fn is_disabled(&self) -> bool {
        self.on_first::<bool>(DISABLED_JS).await.unwrap_or(false)
    }

    /// The first match's box, `None` when there is no match or it has no size.
    pub async fn bounding_box(&self) -> WebResult<Option<Rect>> {
        match self.on_first::<Rect>(BOX_JS).await {
            Ok(r) if r.width > 0.0 || r.height > 0.0 => Ok(Some(r)),
            Ok(_) | Err(WebError::NotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// `js_fn` (`(el) => …`) on the first match, in the isolated world.
    pub async fn evaluate<T: DeserializeOwned>(&self, js_fn: &str) -> WebResult<T> {
        self.on_first(&format!("function () {{ return ({js_fn})(this); }}"))
            .await
    }

    pub async fn input_value(&self) -> WebResult<String> {
        self.on_first("function () { return this.value ?? ''; }")
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::super::testkit::{FixtureServer, TestBrowser};
    use super::*;

    #[test]
    fn name_matching_follows_playwright() {
        assert!(Name::from("create  API").matches("  Create api\n key "));
        assert!(!Name::from("create key").matches("Create API key"));
        let re = Name::from(Regex::new(r"(?i)^\s*continue\s*$").unwrap());
        assert!(re.matches(" Continue "));
        assert!(!re.matches("Continue with Google"));
    }

    #[test]
    fn js_regex_moves_leading_flags() {
        assert_eq!(js_regex(r"(?i)^a\s+b$"), (r"^a\s+b$".into(), "i".into()));
        assert_eq!(js_regex("(?:x)"), ("(?:x)".into(), "".into()));
        assert_eq!(js_regex("plain"), ("plain".into(), "".into()));
    }

    async fn roles() -> Option<(TestBrowser, FixtureServer, Page)> {
        let b = TestBrowser::launch().await?;
        let site = FixtureServer::start().await;
        let page = b.page().await;
        page.goto(&format!("{}/roles.html", site.site))
            .await
            .unwrap();
        Some((b, site, page))
    }

    #[tokio::test]
    async fn role_name_is_case_insensitive_substring() {
        let Some((_b, _s, page)) = roles().await else {
            return;
        };
        assert_eq!(page.role("button", "create api").count().await.unwrap(), 2);
        // ARIA-hidden (display: none) is out for roles, as in Playwright; css sees it.
        assert_eq!(page.css("button.gone").count().await.unwrap(), 1);
        assert!(!page.css("button.gone").is_visible().await);
    }

    #[tokio::test]
    async fn role_skips_the_page_behind_a_modal() {
        let Some((_b, _s, page)) = roles().await else {
            return;
        };
        let loc = page.role("button", "Save changes");
        assert_eq!(loc.count().await.unwrap(), 1);
        assert_eq!(
            loc.evaluate::<String>("(el) => el.id").await.unwrap(),
            "modal-save"
        );
        assert_eq!(page.css("button.save").count().await.unwrap(), 4);
    }

    #[tokio::test]
    async fn nested_scopes_give_each_element_once() {
        let Some((_b, _s, page)) = roles().await else {
            return;
        };
        // body and the divs inside it both hold the buttons.
        let loc = page.css("body, div").css("button.save");
        assert_eq!(loc.count().await.unwrap(), 4);
        let ids = loc.evaluate::<String>("(el) => el.id").await.unwrap();
        assert_eq!(ids, "bg-save");
        assert_eq!(
            loc.last()
                .evaluate::<String>("(el) => el.id")
                .await
                .unwrap(),
            "modal-save"
        );
    }

    #[tokio::test]
    async fn role_regex_name() {
        let Some((_b, _s, page)) = roles().await else {
            return;
        };
        let re = Regex::new(r"(?i)^\s*continue\s*$").unwrap();
        let loc = page.role("button", re);
        assert_eq!(loc.count().await.unwrap(), 1);
        assert_eq!(
            loc.evaluate::<String>("(el) => el.id").await.unwrap(),
            "continue"
        );
    }

    #[tokio::test]
    async fn first_visible_skips_hidden_copy() {
        let Some((_b, _s, page)) = roles().await else {
            return;
        };
        let loc = page.role("button", "create api key");
        assert!(!loc.first().is_visible().await);
        let shown = loc.first_visible().await.unwrap().expect("a visible copy");
        assert!(shown.is_visible().await);
        assert_eq!(
            shown.evaluate::<String>("(el) => el.id").await.unwrap(),
            "create-shown"
        );
        assert!(page
            .css("#nothing")
            .first_visible()
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn scope_to_dialog() {
        let Some((_b, _s, page)) = roles().await else {
            return;
        };
        assert_eq!(page.role("button", "Create key").count().await.unwrap(), 2);
        let dialog = page.role("dialog", None);
        assert_eq!(
            dialog.role("button", "Create key").count().await.unwrap(),
            1
        );
        assert_eq!(dialog.css("input[name=name]").count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn is_disabled_reads_disabled_and_aria_disabled() {
        let Some((_b, _s, page)) = roles().await else {
            return;
        };
        assert!(page.role("button", "Skip question").is_disabled().await);
        assert!(page.css("#aria-off").is_disabled().await);
        assert!(!page.role("button", "Start creating").is_disabled().await);
    }

    #[tokio::test]
    async fn aria_label_counts_as_name() {
        let Some((_b, _s, page)) = roles().await else {
            return;
        };
        assert_eq!(page.role("button", "close").count().await.unwrap(), 1);
        // A link styled as a button, with role=button.
        assert_eq!(page.role("button", "Try again").count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn text_and_filter_text() {
        let Some((_b, _s, page)) = roles().await else {
            return;
        };
        assert_eq!(page.text("check your email").count().await.unwrap(), 1);
        let re = Regex::new(r"(?i)check\s+YOUR").unwrap();
        assert_eq!(page.text(re).count().await.unwrap(), 1);
        assert_eq!(page.css("li").count().await.unwrap(), 3);
        assert_eq!(page.css("li").filter_text("brz_").count().await.unwrap(), 1);
        let ul = page.css("ul");
        assert_eq!(ul.text("brz_").count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn last_picks_the_last_match() {
        let Some((_b, _s, page)) = roles().await else {
            return;
        };
        let last = page.css("li").last();
        assert_eq!(
            last.evaluate::<String>("(el) => el.textContent")
                .await
                .unwrap(),
            "third"
        );
        assert_eq!(
            page.css("li")
                .nth(1)
                .evaluate::<String>("(el) => el.textContent")
                .await
                .unwrap(),
            "brz_123 key"
        );
        assert_eq!(page.css("li").nth(9).count().await.unwrap(), 0);
        assert_eq!(
            page.css("input[name=name]").input_value().await.unwrap(),
            "prefilled"
        );
        let b = page
            .role("button", "Start creating")
            .bounding_box()
            .await
            .unwrap()
            .unwrap();
        assert!(b.width > 0.0 && b.height > 0.0);
        assert!(page.css("#nothing").bounding_box().await.unwrap().is_none());
    }
}
