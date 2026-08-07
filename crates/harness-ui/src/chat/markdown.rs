use super::ChatView;
use super::code_extensions::code_file_extension;
use crate::theme::{Theme, ThemeMode, web_ease_out};
use crate::zoom::px;
use ::markdown::{ParseOptions, mdast::Node};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures::AsyncReadExt as _;
use gpui::{
    Animation, AnimationExt, AnyElement, App, ClipboardItem, Entity, FontWeight, Image,
    ImageFormat, ImageSource, ObjectFit, SharedString, StyleRefinement, Styled, StyledImage,
    StyledText, Window, div, img, prelude::*, relative, rems, svg,
};
use gpui_component::highlighter::SyntaxHighlighter;
use gpui_component::scroll::ScrollableElement;
use gpui_component::text::{TextView, TextViewStyle};
use gpui_component::{ActiveTheme, Rope};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const STREAM_WORD_DURATION_MS: u64 = 160;
const STREAM_WORD_STAGGER_MS: u64 = 14;
const STREAM_CLEANUP_PADDING: Duration = Duration::from_millis(80);
const SPACE_WIDTH: f32 = 3.7;

#[derive(Clone, Debug)]
pub(super) struct StreamRevealBatch {
    generation: u64,
    from: usize,
    to: usize,
    expires_at: Instant,
}

impl ChatView {
    pub(super) fn reset_stream_reveals(&mut self) {
        self.stream_reveal_generation = self.stream_reveal_generation.wrapping_add(1);
        self.stream_reveal_batches.clear();
    }

    pub(super) fn start_stream_reveal(
        &mut self,
        item_id: String,
        from: usize,
        to: usize,
        text: &str,
        cx: &mut gpui::Context<Self>,
    ) {
        if from >= to {
            return;
        }
        let word_count = text.split_whitespace().count().max(1) as u64;
        let duration = stream_batch_duration(word_count);
        self.stream_reveal_generation = self.stream_reveal_generation.wrapping_add(1);
        self.stream_reveal_batches
            .entry(item_id)
            .or_default()
            .push(StreamRevealBatch {
                generation: self.stream_reveal_generation,
                from,
                to,
                expires_at: Instant::now() + duration + STREAM_CLEANUP_PADDING,
            });
        self.schedule_stream_reveal_cleanup(cx);
    }

    fn schedule_stream_reveal_cleanup(&mut self, cx: &mut gpui::Context<Self>) {
        if self.stream_reveal_cleanup_scheduled || self.stream_reveal_batches.is_empty() {
            return;
        }
        self.stream_reveal_cleanup_scheduled = true;
        cx.spawn(async move |view, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(STREAM_WORD_DURATION_MS))
                .await;
            let _ = view.update(cx, |this, cx| {
                this.stream_reveal_cleanup_scheduled = false;
                let now = Instant::now();
                let before = this
                    .stream_reveal_batches
                    .values()
                    .map(Vec::len)
                    .sum::<usize>();
                this.stream_reveal_batches.retain(|_, batches| {
                    batches.retain(|batch| batch.expires_at > now);
                    !batches.is_empty()
                });
                let after = this
                    .stream_reveal_batches
                    .values()
                    .map(Vec::len)
                    .sum::<usize>();
                if before != after {
                    cx.notify();
                }
                this.schedule_stream_reveal_cleanup(cx);
            });
        })
        .detach();
    }

    pub(super) fn close_markdown_table_overlay(&mut self, cx: &mut gpui::Context<Self>) {
        if self.markdown_table_overlay.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn markdown_table_overlay(
        &self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Option<AnyElement> {
        let overlay = self.markdown_table_overlay.as_ref()?;
        let definitions = HashMap::new();
        let view = cx.entity();
        let context = RenderContext {
            id: "markdown-table-fullscreen",
            raw: "",
            streaming: false,
            reveals: &[],
            definitions: &definitions,
            theme: self.theme,
            view: view.clone(),
            reveal_ordinals: RefCell::new(HashMap::new()),
        };
        let rows = render_table_rows(&overlay.table, &context);
        let group: SharedString = "markdown-table-fullscreen-group".into();
        let controls_state = window.use_keyed_state(
            SharedString::from(format!(
                "markdown-table-fullscreen-controls-state:{}",
                overlay.id
            )),
            cx,
            |_, _| TableControlsState::default(),
        );
        let fullscreen_id = format!("markdown-table-fullscreen:{}", overlay.id);
        let controls = table_controls(
            &fullscreen_id,
            overlay.data.clone(),
            TableControlStyle {
                group: group.clone(),
                theme: self.theme,
                enabled: true,
            },
            controls_state,
            None,
            cx,
        );
        let close = table_control_button(
            "markdown-table-fullscreen-close".into(),
            "icons/x.svg",
            group.clone(),
            self.theme,
            true,
            true,
            move |cx| {
                view.update(cx, |this, cx| this.close_markdown_table_overlay(cx));
            },
        );

        Some(
            div()
                .id("markdown-table-fullscreen")
                .group(group)
                .occlude()
                .absolute()
                .inset(px(0.0))
                .flex()
                .flex_col()
                .bg(self.theme.background.hsla())
                .child(
                    div()
                        .h(px(56.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(3.0))
                        .px(px(16.0))
                        .child(controls)
                        .child(close),
                )
                .child(
                    div()
                        .id("markdown-table-fullscreen-scroll")
                        .flex_1()
                        .min_h(px(0.0))
                        .overflow_scroll()
                        .px(px(16.0))
                        .pb(px(16.0))
                        .text_size(px(12.5))
                        .child(div().w_full().children(rows)),
                )
                .into_any_element(),
        )
    }
}

fn stream_batch_duration(word_count: u64) -> Duration {
    Duration::from_millis(
        STREAM_WORD_DURATION_MS
            + STREAM_WORD_STAGGER_MS.saturating_mul(word_count.saturating_sub(1)),
    )
}

pub(super) fn markdown_view(
    id: String,
    text: String,
    streaming: bool,
    reveals: &[StreamRevealBatch],
    view: Entity<ChatView>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let theme = view.read(cx).theme;
    let parsed = ::markdown::to_mdast(&text, &ParseOptions::gfm());
    let Ok(Node::Root(root)) = parsed else {
        return fallback_markdown(id, text, theme, window, cx);
    };
    let definitions = collect_definitions(&root.children);
    let context = RenderContext {
        id: &id,
        raw: &text,
        streaming,
        reveals,
        definitions: &definitions,
        theme,
        view,
        reveal_ordinals: RefCell::new(HashMap::new()),
    };
    let mut blocks = Vec::with_capacity(root.children.len());
    let visible_count = root
        .children
        .iter()
        .filter(|node| !matches!(node, Node::Definition(_)))
        .count();
    let mut visible_index = 0;
    for node in &root.children {
        if matches!(node, Node::Definition(_)) {
            continue;
        }
        blocks.push(render_block(
            node,
            visible_index == 0,
            visible_index + 1 == visible_count,
            0,
            &context,
            window,
            cx,
        ));
        visible_index += 1;
    }

    div()
        .id(SharedString::from(id))
        .w_full()
        .max_w(px(690.0))
        .font_family("Geist")
        .text_size(px(15.0))
        .line_height(relative(1.52))
        .text_color(theme.response_text.hsla())
        .children(blocks)
        .into_any_element()
}

struct RenderContext<'a> {
    id: &'a str,
    raw: &'a str,
    streaming: bool,
    reveals: &'a [StreamRevealBatch],
    definitions: &'a HashMap<String, String>,
    theme: Theme,
    view: Entity<ChatView>,
    reveal_ordinals: RefCell<HashMap<u64, usize>>,
}

fn render_block(
    node: &Node,
    first: bool,
    last: bool,
    depth: usize,
    context: &RenderContext<'_>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    match node {
        Node::Paragraph(paragraph) if contains_media(&paragraph.children) => {
            render_media_paragraph(paragraph, last, context, window, cx)
        }
        Node::Paragraph(paragraph) if !contains_media(&paragraph.children) => div()
            .w_full()
            .when(!last, |paragraph| paragraph.mb(px(8.0)))
            .child(render_inline_flow(&paragraph.children, context))
            .into_any_element(),
        Node::Heading(heading) if !contains_media(&heading.children) => {
            let size = match heading.depth {
                1 => 20.0,
                2 => 15.0,
                _ => 13.5,
            };
            div()
                .w_full()
                .when(!first, |heading| heading.mt(px(18.0)))
                .when(!last, |heading| heading.mb(px(6.0)))
                .font_weight(FontWeight(580.0))
                .text_size(px(size))
                .line_height(relative(1.3))
                .child(render_inline_flow(&heading.children, context))
                .into_any_element()
        }
        Node::Blockquote(blockquote) => {
            let mut children = Vec::with_capacity(blockquote.children.len());
            for (index, child) in blockquote.children.iter().enumerate() {
                children.push(render_block(
                    child,
                    index == 0,
                    index + 1 == blockquote.children.len(),
                    depth + 1,
                    context,
                    window,
                    cx,
                ));
            }
            div()
                .w_full()
                .when(!last, |quote| quote.mb(px(8.0)))
                .border_l(px(2.0))
                .border_color(context.theme.line_strong.hsla())
                .pl(px(12.0))
                .text_color(context.theme.text_2.hsla())
                .children(children)
                .into_any_element()
        }
        Node::List(list) => render_list(list, last, depth, context, window, cx),
        Node::Code(code) => render_code_block(code, last, context, window, cx),
        Node::Math(math) => render_code_block(
            &::markdown::mdast::Code {
                value: math.value.clone(),
                position: math.position.clone(),
                lang: None,
                meta: None,
            },
            last,
            context,
            window,
            cx,
        ),
        Node::Table(table) => render_table(table, last, context, window, cx),
        Node::ThematicBreak(_) => div()
            .w_full()
            .h(px(1.0))
            .when(!first, |rule| rule.mt(px(14.0)))
            .when(!last, |rule| rule.mb(px(14.0)))
            .bg(context.theme.line.hsla())
            .into_any_element(),
        Node::Definition(_) => div().into_any_element(),
        _ => fallback_node(node, last, context, window, cx),
    }
}

#[derive(Default)]
struct CopyFeedbackState {
    copied: bool,
    generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TableMenu {
    Copy,
    Download,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TableFormat {
    Markdown,
    Csv,
    Tsv,
}

#[derive(Default)]
struct TableControlsState {
    menu: Option<TableMenu>,
    copied: bool,
    generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TableData {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

struct TableControlStyle {
    group: SharedString,
    theme: Theme,
    enabled: bool,
}

#[derive(Clone)]
pub(super) struct MarkdownTableOverlay {
    id: String,
    table: ::markdown::mdast::Table,
    data: TableData,
}

type TableAction = Rc<dyn Fn(&mut App)>;

fn render_media_paragraph(
    paragraph: &::markdown::mdast::Paragraph,
    last: bool,
    context: &RenderContext<'_>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let mut parts = Vec::new();
    let mut inline = Vec::new();
    for node in &paragraph.children {
        if contains_media(std::slice::from_ref(node)) {
            if !inline.is_empty() {
                parts.push(render_inline_flow_sized(&inline, context, false));
                inline.clear();
            }
            parts.push(render_media_node(node, None, context, window, cx));
        } else {
            inline.push(node.clone());
        }
    }
    if !inline.is_empty() {
        parts.push(render_inline_flow_sized(&inline, context, false));
    }

    div()
        .w_full()
        .flex()
        .flex_wrap()
        .items_center()
        .when(!last, |paragraph| paragraph.mb(px(8.0)))
        .children(parts)
        .into_any_element()
}

fn render_media_node(
    node: &Node,
    link: Option<String>,
    context: &RenderContext<'_>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    match node {
        Node::Image(image) => render_markdown_image(
            &image.url,
            &image.alt,
            link,
            node_start(node).unwrap_or_default(),
            context,
            cx,
        ),
        Node::ImageReference(image) => context.definitions.get(&image.identifier).map_or_else(
            || image_fallback(context.theme),
            |url| {
                render_markdown_image(
                    url,
                    &image.alt,
                    link,
                    node_start(node).unwrap_or_default(),
                    context,
                    cx,
                )
            },
        ),
        Node::Link(node) => {
            render_media_children(&node.children, Some(node.url.clone()), context, window, cx)
        }
        Node::LinkReference(node) => render_media_children(
            &node.children,
            context.definitions.get(&node.identifier).cloned(),
            context,
            window,
            cx,
        ),
        Node::Strong(node) => render_media_children(&node.children, link, context, window, cx),
        Node::Emphasis(node) => render_media_children(&node.children, link, context, window, cx),
        Node::Delete(node) => render_media_children(&node.children, link, context, window, cx),
        _ => render_inline_flow_sized(std::slice::from_ref(node), context, false),
    }
}

fn render_media_children(
    nodes: &[Node],
    link: Option<String>,
    context: &RenderContext<'_>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    div()
        .w_auto()
        .max_w(relative(1.0))
        .flex()
        .flex_wrap()
        .children(
            nodes
                .iter()
                .map(|node| render_media_node(node, link.clone(), context, window, cx)),
        )
        .into_any_element()
}

fn render_markdown_image(
    url: &str,
    alt: &str,
    link: Option<String>,
    start: usize,
    context: &RenderContext<'_>,
    cx: &mut App,
) -> AnyElement {
    if url.is_empty() {
        return div().into_any_element();
    }
    let project_path = context
        .view
        .read(cx)
        .session
        .as_ref()
        .map(|session| session.project_path.clone());
    let Some(source) = markdown_image_source(url, project_path.as_deref()) else {
        return image_fallback(context.theme);
    };
    let id = format!("{}:image:{start}", context.id);
    let group: SharedString = format!("markdown-image-group:{id}").into();
    let fallback_theme = context.theme;
    let image = img(source)
        .id(SharedString::from(format!("{id}:content")))
        .max_w(relative(1.0))
        .rounded(px(8.0))
        .object_fit(ObjectFit::Contain)
        .with_fallback(move || image_fallback(fallback_theme));
    let source_url = url.to_owned();
    let source_alt = alt.to_owned();
    let download_project_path = project_path.clone();
    let download_view = context.view.clone();

    div()
        .id(SharedString::from(format!("{id}:wrapper")))
        .group(group.clone())
        .relative()
        .w_auto()
        .max_w(relative(1.0))
        .flex()
        .overflow_hidden()
        .rounded(px(8.0))
        .when_some(link, |wrapper, link| {
            wrapper
                .cursor_pointer()
                .on_click(move |_event, _window, cx| cx.open_url(&link))
        })
        .child(image)
        .child(
            div()
                .absolute()
                .inset(px(0.0))
                .rounded(px(8.0))
                .bg(gpui::black().opacity(0.1))
                .opacity(0.0)
                .group_hover(group.clone(), |overlay| overlay.opacity(1.0)),
        )
        .child(
            div()
                .id(SharedString::from(format!("{id}:download")))
                .absolute()
                .right(px(8.0))
                .bottom(px(8.0))
                .size(px(32.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .border_1()
                .border_color(context.theme.line.hsla())
                .bg(context.theme.background.hsla().opacity(0.9))
                .text_color(context.theme.text_2.hsla())
                .shadow_md()
                .opacity(0.0)
                .group_hover(group, |button| button.opacity(1.0))
                .cursor_pointer()
                .hover(move |button| {
                    button
                        .bg(context.theme.background.hsla())
                        .text_color(context.theme.text.hsla())
                })
                .on_click(move |_event, _window, cx| {
                    cx.stop_propagation();
                    download_markdown_image(
                        source_url.clone(),
                        source_alt.clone(),
                        download_project_path.clone(),
                        download_view.clone(),
                        cx,
                    );
                })
                .child(svg().path("icons/download.svg").size(px(14.0))),
        )
        .into_any_element()
}

fn image_fallback(theme: Theme) -> AnyElement {
    div()
        .text_size(px(11.0))
        .italic()
        .text_color(theme.text_3.hsla())
        .child("Image not available")
        .into_any_element()
}

fn markdown_image_source(url: &str, project_path: Option<&str>) -> Option<ImageSource> {
    if url.starts_with("data:") {
        let (format, bytes) = decode_data_image(url)?;
        return Some(Arc::new(Image::from_bytes(format, bytes)).into());
    }
    if let Some(path) = markdown_image_path(url, project_path) {
        return Some(path.into());
    }
    if url.starts_with("//") {
        return Some(format!("https:{url}").into());
    }
    let parsed = url::Url::parse(url).ok()?;
    matches!(parsed.scheme(), "http" | "https").then(|| url.to_owned().into())
}

fn markdown_image_path(url: &str, project_path: Option<&str>) -> Option<PathBuf> {
    if url
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
    {
        return url::Url::parse(url).ok()?.to_file_path().ok();
    }
    let decoded = percent_decode(url).unwrap_or_else(|| url.to_owned());
    let path = decoded.split(['?', '#']).next().unwrap_or(decoded.as_str());
    if Path::new(path).is_absolute() || path.starts_with("\\\\") || is_windows_absolute_path(path) {
        return Some(PathBuf::from(path));
    }
    let external = url::Url::parse(url)
        .ok()
        .is_some_and(|url| !url.scheme().is_empty());
    if external || path.is_empty() {
        return None;
    }
    project_path.map(|project_path| Path::new(project_path).join(path))
}

fn decode_data_image(source: &str) -> Option<(ImageFormat, Vec<u8>)> {
    let (metadata, payload) = source.strip_prefix("data:")?.split_once(',')?;
    let mime = metadata.split(';').next().unwrap_or_default();
    let format = ImageFormat::from_mime_type(mime)?;
    let bytes = if metadata
        .split(';')
        .any(|part| part.eq_ignore_ascii_case("base64"))
    {
        STANDARD.decode(payload).ok()?
    } else {
        percent_decode_bytes(payload)?
    };
    Some((format, bytes))
}

fn download_markdown_image(
    source: String,
    alt: String,
    project_path: Option<String>,
    view: Entity<ChatView>,
    cx: &mut App,
) {
    let client = cx.http_client();
    let fallback_source = source.clone();
    let load = cx.background_spawn(async move {
        markdown_image_bytes(&source, project_path.as_deref(), client).await
    });
    cx.spawn(async move |cx| match load.await {
        Ok(download) => {
            let filename = image_download_filename(&fallback_source, &alt, download.extension);
            let _ = view.update(cx, |_this, cx| {
                prompt_download_bytes(filename, download.bytes, cx);
            });
        }
        Err(_) if is_external_image_url(&fallback_source) => {
            let _ = view.update(cx, |_this, cx| cx.open_url(&fallback_source));
        }
        Err(_) => {}
    })
    .detach();
}

struct DownloadedImage {
    bytes: Vec<u8>,
    extension: Option<&'static str>,
}

async fn markdown_image_bytes(
    source: &str,
    project_path: Option<&str>,
    client: Arc<dyn gpui::http_client::HttpClient>,
) -> anyhow::Result<DownloadedImage> {
    if source.starts_with("data:") {
        return decode_data_image(source)
            .map(|(format, bytes)| DownloadedImage {
                bytes,
                extension: Some(image_format_extension(format)),
            })
            .ok_or_else(|| anyhow::anyhow!("invalid image data URL"));
    }
    if let Some(path) = markdown_image_path(source, project_path) {
        let bytes = std::fs::read(path)?;
        let extension = detect_image_extension(&bytes);
        return Ok(DownloadedImage { bytes, extension });
    }
    let request_url = if source.starts_with("//") {
        format!("https:{source}")
    } else {
        source.to_owned()
    };
    if !is_external_image_url(&request_url) {
        anyhow::bail!("unsupported image URL");
    }
    let mut response = client.get(&request_url, ().into(), true).await?;
    if !response.status().is_success() {
        anyhow::bail!("image request failed with {}", response.status());
    }
    let extension = response
        .headers()
        .get(gpui::http_client::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(image_mime_extension);
    let mut bytes = Vec::new();
    response.body_mut().read_to_end(&mut bytes).await?;
    let extension = extension.or_else(|| detect_image_extension(&bytes));
    Ok(DownloadedImage { bytes, extension })
}

fn is_external_image_url(source: &str) -> bool {
    url::Url::parse(source)
        .ok()
        .is_some_and(|url| matches!(url.scheme(), "http" | "https"))
}

fn image_download_filename(source: &str, alt: &str, detected_extension: Option<&str>) -> String {
    let source_path = if source.starts_with("data:") {
        String::new()
    } else {
        url::Url::parse(source)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https" | "file"))
            .map(|url| url.path().to_owned())
            .unwrap_or_else(|| source.split(['?', '#']).next().unwrap_or(source).to_owned())
    };
    let segment = source_path
        .rsplit(['/', '\\'])
        .next()
        .and_then(|segment| percent_decode(segment).or_else(|| Some(segment.to_owned())))
        .unwrap_or_default();
    if let Some((_, extension)) = segment.rsplit_once('.')
        && !extension.is_empty()
        && extension.len() <= 4
    {
        return sanitize_download_name(&segment);
    }
    let extension = detected_extension
        .or_else(|| {
            source
                .strip_prefix("data:")
                .and_then(|data| data.split([';', ',']).next())
                .and_then(image_mime_extension)
        })
        .unwrap_or("png");
    let base = if alt.trim().is_empty() {
        if segment.is_empty() {
            "image"
        } else {
            &segment
        }
    } else {
        alt.trim()
    };
    let base = base
        .rsplit_once('.')
        .map_or(base, |(without_extension, _)| without_extension);
    format!("{}.{}", sanitize_download_name(base), extension)
}

fn image_mime_extension(mime: &str) -> Option<&'static str> {
    match mime.split(';').next()?.trim().to_ascii_lowercase().as_str() {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        "image/svg+xml" => Some("svg"),
        "image/bmp" => Some("bmp"),
        "image/tiff" | "image/tif" => Some("tiff"),
        "image/avif" => Some("avif"),
        "image/x-icon" | "image/vnd.microsoft.icon" => Some("ico"),
        _ => None,
    }
}

fn detect_image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("webp")
    } else if bytes.starts_with(b"BM") {
        Some("bmp")
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        Some("tiff")
    } else if bytes.get(4..12) == Some(b"ftypavif") {
        Some("avif")
    } else if std::str::from_utf8(bytes)
        .ok()
        .map(str::trim_start)
        .is_some_and(|text| text.starts_with("<svg") || text.starts_with("<?xml"))
    {
        Some("svg")
    } else {
        None
    }
}

fn image_format_extension(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Webp => "webp",
        ImageFormat::Gif => "gif",
        ImageFormat::Svg => "svg",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Tiff => "tiff",
    }
}

fn sanitize_download_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_control() || matches!(character, '/' | '\\' | ':') {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    if sanitized.trim().is_empty() {
        "image".into()
    } else {
        sanitized
    }
}

fn render_code_block(
    code: &::markdown::mdast::Code,
    last: bool,
    context: &RenderContext<'_>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let start = code
        .position
        .as_ref()
        .map_or(0, |position| position.start.offset);
    let block_id = format!("{}:code:{start}", context.id);
    let group: SharedString = format!("markdown-code-group:{block_id}").into();
    let highlights = code.lang.as_deref().map_or_else(Vec::new, |language| {
        let rope = Rope::from(code.value.as_str());
        let mut highlighter = SyntaxHighlighter::new(language);
        highlighter.update(None, &rope);
        highlighter.styles(&(0..code.value.len()), &cx.theme().highlight_theme)
    });
    let content = if highlights.is_empty() {
        StyledText::new(code.value.clone())
    } else {
        StyledText::new(code.value.clone()).with_highlights(highlights)
    };
    let copy = copy_control(
        format!("{block_id}:copy"),
        code.value.clone(),
        group.clone(),
        context.theme,
        !context.streaming,
        window,
        cx,
    );
    let filename = format!(
        "file.{}",
        code.lang.as_deref().map_or("txt", code_file_extension)
    );
    let download = download_control(
        format!("{block_id}:download"),
        filename,
        code.value.clone(),
        group.clone(),
        context.theme,
        !context.streaming,
    );

    div()
        .group(group)
        .w_full()
        .when(!last, |block| block.mb(px(14.0)))
        .child(
            div()
                .h(px(26.0))
                .px(px(2.0))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .font_family("Geist Mono")
                .text_size(px(11.5))
                .text_color(context.theme.text_3.hsla())
                .child(code.lang.clone().unwrap_or_default())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(download)
                        .child(copy),
                ),
        )
        .child(
            div()
                .id(SharedString::from(format!("{block_id}:scroll")))
                .w_full()
                .overflow_x_scrollbar()
                .rounded(px(8.0))
                .border_1()
                .border_color(context.theme.line.hsla())
                .bg(context.theme.surface.hsla())
                .px(px(13.0))
                .py(px(11.0))
                .font_family("Geist Mono")
                .text_size(px(12.5))
                .line_height(relative(1.52))
                .text_color(context.theme.response_text.hsla())
                .whitespace_nowrap()
                .child(content),
        )
        .into_any_element()
}

fn render_table(
    table: &::markdown::mdast::Table,
    last: bool,
    context: &RenderContext<'_>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let start = table
        .position
        .as_ref()
        .map_or(0, |position| position.start.offset);
    let table_id = format!("{}:table:{start}", context.id);
    let group: SharedString = format!("markdown-table-group:{table_id}").into();
    let controls_state = window.use_keyed_state(
        SharedString::from(format!("{table_id}:controls-state")),
        cx,
        |_, _| TableControlsState::default(),
    );
    let data = table_data(table);
    let overlay = MarkdownTableOverlay {
        id: table_id.clone(),
        table: table.clone(),
        data: data.clone(),
    };
    let view = context.view.clone();
    let fullscreen_action: TableAction = Rc::new(move |cx| {
        view.update(cx, |this, cx| {
            this.markdown_table_overlay = Some(overlay.clone());
            cx.notify();
        });
    });
    let controls = table_controls(
        &table_id,
        data,
        TableControlStyle {
            group: group.clone(),
            theme: context.theme,
            enabled: !context.streaming,
        },
        controls_state,
        Some(fullscreen_action),
        cx,
    );
    let rows = render_table_rows(table, context);

    div()
        .group(group)
        .relative()
        .w_full()
        .when(!last, |table| table.mb(px(14.0)))
        .text_size(px(12.5))
        .child(
            div()
                .id(SharedString::from(format!("{table_id}:scroll")))
                .w_full()
                .overflow_x_scrollbar()
                .children(rows),
        )
        .child(div().absolute().top(px(3.0)).right(px(3.0)).child(controls))
        .into_any_element()
}

fn render_table_rows(
    table: &::markdown::mdast::Table,
    context: &RenderContext<'_>,
) -> Vec<AnyElement> {
    let column_count = table
        .children
        .iter()
        .filter_map(|row| match row {
            Node::TableRow(row) => Some(row.children.len()),
            _ => None,
        })
        .max()
        .unwrap_or_default();
    let mut rows = Vec::with_capacity(table.children.len());
    for (row_index, row) in table.children.iter().enumerate() {
        let Node::TableRow(row) = row else {
            continue;
        };
        let mut cells = Vec::with_capacity(row.children.len());
        for (column_index, cell) in row.children.iter().enumerate() {
            let Node::TableCell(cell) = cell else {
                continue;
            };
            cells.push(
                div()
                    .min_w(px(120.0))
                    .flex_1()
                    .border_l_1()
                    .border_b_1()
                    .when(row_index == 0, |cell| cell.border_t_1())
                    .when(column_index + 1 == row.children.len(), |cell| {
                        cell.border_r_1()
                    })
                    .border_color(context.theme.line.hsla())
                    .when(row_index == 0, |cell| {
                        cell.bg(context.theme.surface.hsla())
                            .font_weight(FontWeight(540.0))
                    })
                    .px(px(9.0))
                    .py(px(5.0))
                    .child(render_inline_flow(&cell.children, context)),
            );
        }
        rows.push(
            div()
                .w_full()
                .min_w(px(column_count as f32 * 120.0))
                .flex()
                .children(cells)
                .into_any_element(),
        );
    }
    rows
}

fn table_controls(
    table_id: &str,
    data: TableData,
    style: TableControlStyle,
    state: Entity<TableControlsState>,
    fullscreen_action: Option<TableAction>,
    cx: &mut App,
) -> AnyElement {
    let TableControlStyle {
        group,
        theme,
        enabled,
    } = style;
    let open_menu = state.read(cx).menu;
    let copied = state.read(cx).copied;
    let copy_menu = table_menu(
        format!("{table_id}:copy-menu"),
        &[
            ("Markdown", TableFormat::Markdown),
            ("CSV", TableFormat::Csv),
            ("TSV", TableFormat::Tsv),
        ],
        data.clone(),
        false,
        theme,
        state.clone(),
    );
    let download_menu = table_menu(
        format!("{table_id}:download-menu"),
        &[
            ("CSV", TableFormat::Csv),
            ("Markdown", TableFormat::Markdown),
        ],
        data,
        true,
        theme,
        state.clone(),
    );
    let copy_state = state.clone();
    let download_state = state.clone();

    div()
        .flex()
        .items_center()
        .gap(px(3.0))
        .child(
            div()
                .relative()
                .child(table_control_button(
                    format!("{table_id}:copy"),
                    if copied {
                        "icons/check.svg"
                    } else {
                        "icons/copy.svg"
                    },
                    group.clone(),
                    theme,
                    enabled,
                    open_menu == Some(TableMenu::Copy),
                    move |cx| {
                        copy_state.update(cx, |state, cx| {
                            state.menu = if state.menu == Some(TableMenu::Copy) {
                                None
                            } else {
                                Some(TableMenu::Copy)
                            };
                            cx.notify();
                        });
                    },
                ))
                .when(open_menu == Some(TableMenu::Copy), |control| {
                    control.child(copy_menu)
                }),
        )
        .child(
            div()
                .relative()
                .child(table_control_button(
                    format!("{table_id}:download"),
                    "icons/download.svg",
                    group.clone(),
                    theme,
                    enabled,
                    open_menu == Some(TableMenu::Download),
                    move |cx| {
                        download_state.update(cx, |state, cx| {
                            state.menu = if state.menu == Some(TableMenu::Download) {
                                None
                            } else {
                                Some(TableMenu::Download)
                            };
                            cx.notify();
                        });
                    },
                ))
                .when(open_menu == Some(TableMenu::Download), |control| {
                    control.child(download_menu)
                }),
        )
        .when_some(fullscreen_action, |controls, fullscreen_action| {
            controls.child(table_control_button(
                format!("{table_id}:fullscreen"),
                "icons/maximize-2.svg",
                group,
                theme,
                enabled,
                false,
                move |cx| fullscreen_action(cx),
            ))
        })
        .into_any_element()
}

fn table_control_button(
    id: String,
    icon: &'static str,
    group: SharedString,
    theme: Theme,
    enabled: bool,
    open: bool,
    on_click: impl Fn(&mut App) + 'static,
) -> AnyElement {
    div()
        .id(SharedString::from(id))
        .size(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.0))
        .border_1()
        .border_color(theme.line.hsla())
        .bg(theme.surface_2.hsla())
        .text_color(theme.text_3.hsla())
        .opacity(if open { 1.0 } else { 0.0 })
        .group_hover(group, move |control| {
            control.opacity(if enabled { 1.0 } else { 0.5 })
        })
        .when(enabled, |control| {
            control
                .cursor_pointer()
                .hover(move |control| control.text_color(theme.text.hsla()))
                .on_click(move |_event, _window, cx| {
                    cx.stop_propagation();
                    on_click(cx);
                })
        })
        .child(svg().path(icon).size(px(13.0)))
        .into_any_element()
}

fn table_menu(
    id: String,
    options: &[(&'static str, TableFormat)],
    data: TableData,
    download: bool,
    theme: Theme,
    state: Entity<TableControlsState>,
) -> AnyElement {
    div()
        .id(SharedString::from(id.clone()))
        .occlude()
        .absolute()
        .top(px(28.0))
        .right(px(0.0))
        .w(px(120.0))
        .overflow_hidden()
        .rounded(px(6.0))
        .border_1()
        .border_color(theme.line.hsla())
        .bg(theme.background.hsla())
        .shadow_lg()
        .py(px(3.0))
        .children(options.iter().enumerate().map(|(index, (label, format))| {
            let state = state.clone();
            let data = data.clone();
            let label = *label;
            let format = *format;
            div()
                .id(SharedString::from(format!("{id}:option:{index}")))
                .h(px(30.0))
                .w_full()
                .flex()
                .items_center()
                .px(px(9.0))
                .text_size(px(12.5))
                .text_color(theme.text_2.hsla())
                .cursor_pointer()
                .hover(move |option| {
                    option
                        .bg(theme.surface_2.hsla())
                        .text_color(theme.text.hsla())
                })
                .on_click(move |_event, _window, cx| {
                    cx.stop_propagation();
                    let value = format_table(&data, format);
                    if download {
                        let (filename, value) = match format {
                            TableFormat::Csv => ("table.csv", format!("\u{feff}{value}")),
                            TableFormat::Markdown => ("table.md", value),
                            TableFormat::Tsv => ("table.tsv", value),
                        };
                        prompt_download(filename.to_owned(), value, cx);
                        state.update(cx, |state, cx| {
                            state.menu = None;
                            cx.notify();
                        });
                    } else {
                        cx.write_to_clipboard(ClipboardItem::new_string(value));
                        state.update(cx, |state, cx| {
                            state.menu = None;
                            state.copied = true;
                            state.generation = state.generation.wrapping_add(1);
                            cx.notify();
                        });
                        let generation = state.read(cx).generation;
                        let state = state.clone();
                        cx.spawn(async move |cx| {
                            cx.background_executor().timer(Duration::from_secs(2)).await;
                            let _ = state.update(cx, |state, cx| {
                                if state.generation == generation {
                                    state.copied = false;
                                    cx.notify();
                                }
                            });
                        })
                        .detach();
                    }
                })
                .child(label)
        }))
        .into_any_element()
}

fn copy_control(
    id: String,
    value: String,
    group: SharedString,
    theme: Theme,
    enabled: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let state = window.use_keyed_state(SharedString::from(format!("{id}:state")), cx, |_, _| {
        CopyFeedbackState::default()
    });
    let copied = state.read(cx).copied;
    div()
        .id(SharedString::from(id))
        .size(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.0))
        .border_1()
        .border_color(theme.line.hsla())
        .bg(theme.surface_2.hsla())
        .text_color(theme.text_3.hsla())
        .opacity(0.0)
        .group_hover(group, move |control| {
            control.opacity(if enabled { 1.0 } else { 0.5 })
        })
        .when(enabled, |control| {
            control
                .cursor_pointer()
                .hover(move |control| control.text_color(theme.text.hsla()))
                .on_click(move |_event, _window, cx| {
                    cx.stop_propagation();
                    cx.write_to_clipboard(ClipboardItem::new_string(value.clone()));
                    state.update(cx, |state, cx| {
                        state.copied = true;
                        state.generation = state.generation.wrapping_add(1);
                        cx.notify();
                    });
                    let generation = state.read(cx).generation;
                    let state = state.clone();
                    cx.spawn(async move |cx| {
                        cx.background_executor().timer(Duration::from_secs(2)).await;
                        let _ = state.update(cx, |state, cx| {
                            if state.generation == generation {
                                state.copied = false;
                                cx.notify();
                            }
                        });
                    })
                    .detach();
                })
        })
        .child(
            svg()
                .path(if copied {
                    "icons/check.svg"
                } else {
                    "icons/copy.svg"
                })
                .size(px(13.0)),
        )
        .into_any_element()
}

fn download_control(
    id: String,
    filename: String,
    value: String,
    group: SharedString,
    theme: Theme,
    enabled: bool,
) -> AnyElement {
    div()
        .id(SharedString::from(id))
        .size(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.0))
        .border_1()
        .border_color(theme.line.hsla())
        .bg(theme.surface_2.hsla())
        .text_color(theme.text_3.hsla())
        .opacity(0.0)
        .group_hover(group, move |control| {
            control.opacity(if enabled { 1.0 } else { 0.5 })
        })
        .when(enabled, |control| {
            control
                .cursor_pointer()
                .hover(move |control| control.text_color(theme.text.hsla()))
                .on_click(move |_event, _window, cx| {
                    cx.stop_propagation();
                    prompt_download(filename.clone(), value.clone(), cx);
                })
        })
        .child(svg().path("icons/download.svg").size(px(13.0)))
        .into_any_element()
}

fn prompt_download(filename: String, value: String, cx: &mut App) {
    prompt_download_bytes(filename, value.into_bytes(), cx);
}

fn prompt_download_bytes(filename: String, value: Vec<u8>, cx: &mut App) {
    let directory = dirs::download_dir().unwrap_or_else(std::env::temp_dir);
    let receiver = cx.prompt_for_new_path(&directory, Some(&filename));
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(destination))) = receiver.await else {
            return;
        };
        let write = cx.background_spawn(async move { std::fs::write(destination, value) });
        let _ = write.await;
    })
    .detach();
}

fn table_data(table: &::markdown::mdast::Table) -> TableData {
    let mut rows = table.children.iter().filter_map(|row| match row {
        Node::TableRow(row) => Some(
            row.children
                .iter()
                .filter_map(|cell| match cell {
                    Node::TableCell(cell) => {
                        Some(flatten_inline_text(&cell.children).trim().to_owned())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>(),
        ),
        _ => None,
    });
    TableData {
        headers: rows.next().unwrap_or_default(),
        rows: rows.collect(),
    }
}

fn format_table(data: &TableData, format: TableFormat) -> String {
    match format {
        TableFormat::Markdown => format_table_markdown(data),
        TableFormat::Csv => format_table_csv(data),
        TableFormat::Tsv => format_table_tsv(data),
    }
}

fn format_table_csv(data: &TableData) -> String {
    fn escape(value: &str) -> String {
        let quoted = value
            .chars()
            .any(|character| matches!(character, '"' | ',' | '\n'));
        if !quoted {
            return value.to_owned();
        }
        format!("\"{}\"", value.replace('"', "\"\""))
    }

    let mut lines = Vec::with_capacity(data.rows.len() + usize::from(!data.headers.is_empty()));
    if !data.headers.is_empty() {
        lines.push(
            data.headers
                .iter()
                .map(|value| escape(value))
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    lines.extend(data.rows.iter().map(|row| {
        row.iter()
            .map(|value| escape(value))
            .collect::<Vec<_>>()
            .join(",")
    }));
    lines.join("\n")
}

fn format_table_tsv(data: &TableData) -> String {
    fn escape(value: &str) -> String {
        value
            .replace('\t', "\\t")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    }

    let mut lines = Vec::with_capacity(data.rows.len() + usize::from(!data.headers.is_empty()));
    if !data.headers.is_empty() {
        lines.push(
            data.headers
                .iter()
                .map(|value| escape(value))
                .collect::<Vec<_>>()
                .join("\t"),
        );
    }
    lines.extend(data.rows.iter().map(|row| {
        row.iter()
            .map(|value| escape(value))
            .collect::<Vec<_>>()
            .join("\t")
    }));
    lines.join("\n")
}

fn format_table_markdown(data: &TableData) -> String {
    fn escape(value: &str) -> String {
        value.replace('\\', "\\\\").replace('|', "\\|")
    }

    if data.headers.is_empty() {
        return String::new();
    }
    let mut lines = Vec::with_capacity(data.rows.len() + 2);
    lines.push(format!(
        "| {} |",
        data.headers
            .iter()
            .map(|value| escape(value))
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    lines.push(format!(
        "| {} |",
        vec!["---"; data.headers.len()].join(" | ")
    ));
    for row in &data.rows {
        let cells = if row.len() < data.headers.len() {
            (0..data.headers.len())
                .map(|index| row.get(index).map_or(String::new(), |value| escape(value)))
                .collect::<Vec<_>>()
        } else {
            row.iter().map(|value| escape(value)).collect::<Vec<_>>()
        };
        lines.push(format!("| {} |", cells.join(" | ")));
    }
    lines.join("\n")
}

fn render_list(
    list: &::markdown::mdast::List,
    last: bool,
    depth: usize,
    context: &RenderContext<'_>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let mut rows = Vec::with_capacity(list.children.len());
    let start = list.start.unwrap_or(1);
    for (index, child) in list.children.iter().enumerate() {
        let Node::ListItem(item) = child else {
            continue;
        };
        let marker = match item.checked {
            Some(true) => "✓".to_owned(),
            Some(false) => "□".to_owned(),
            None if list.ordered => format!("{}.", start + index as u32),
            None => "•".to_owned(),
        };
        let mut contents = Vec::with_capacity(item.children.len());
        for (child_index, child) in item.children.iter().enumerate() {
            let content = match child {
                Node::Paragraph(paragraph) if !contains_media(&paragraph.children) => {
                    render_inline_flow(&paragraph.children, context).into_any_element()
                }
                _ => render_block(
                    child,
                    child_index == 0,
                    child_index + 1 == item.children.len(),
                    depth + 1,
                    context,
                    window,
                    cx,
                ),
            };
            contents.push(content);
        }
        rows.push(
            div()
                .w_full()
                .flex()
                .items_start()
                .when(index > 0, |row| row.mt(px(1.0)))
                .child(
                    div()
                        .w(px(20.0))
                        .flex_none()
                        .text_color(context.theme.text_3.hsla())
                        .child(marker),
                )
                .child(div().min_w(px(0.0)).flex_1().children(contents)),
        );
    }
    div()
        .w_full()
        .when(depth > 0, |list| list.ml(px(20.0)))
        .when(!last, |list| list.mb(px(8.0)))
        .children(rows)
        .into_any_element()
}

fn fallback_node(
    node: &Node,
    last: bool,
    context: &RenderContext<'_>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let Some(position) = node.position() else {
        return div().into_any_element();
    };
    let Some(raw) = context.raw.get(position.start.offset..position.end.offset) else {
        return div().into_any_element();
    };
    let node_id = format!("{}:fallback:{}", context.id, position.start.offset);
    div()
        .w_full()
        .when(!last && matches!(node, Node::Paragraph(_)), |block| {
            block.mb(px(8.0))
        })
        .when(
            !last && matches!(node, Node::Code(_) | Node::Math(_)),
            |block| block.mb(px(14.0)),
        )
        .child(fallback_markdown(
            node_id,
            raw.to_owned(),
            context.theme,
            window,
            cx,
        ))
        .into_any_element()
}

fn fallback_markdown(
    id: String,
    text: String,
    theme: Theme,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let code_style = StyleRefinement::default()
        .bg(theme.surface.hsla())
        .border_1()
        .border_color(theme.line.hsla())
        .rounded(px(8.0))
        .px(px(13.0))
        .py(px(11.0))
        .font_family("Geist Mono")
        .text_size(px(12.5));
    let mut text_style = TextViewStyle::default()
        .paragraph_gap(rems(0.5))
        .heading_font_size(|level, _base| match level {
            1 => px(20.0),
            2 => px(15.0),
            _ => px(13.5),
        })
        .code_block(code_style);
    text_style.heading_base_font_size = px(15.0);
    text_style.is_dark = theme.mode == ThemeMode::Dark;

    TextView::markdown(SharedString::from(id), text, window, cx)
        .style(text_style)
        .selectable(true)
        .w_full()
        .font_family("Geist")
        .text_size(px(15.0))
        .line_height(relative(1.52))
        .text_color(theme.response_text.hsla())
        .code_block_actions(move |block, _window, _cx| {
            let code = block.code().to_string();
            div()
                .id(SharedString::from(format!(
                    "copy-code:{}",
                    stable_hash(&code)
                )))
                .size(px(24.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.0))
                .border_1()
                .border_color(theme.line.hsla())
                .bg(theme.surface_2.hsla())
                .text_color(theme.text_3.hsla())
                .cursor_pointer()
                .hover(move |style| style.text_color(theme.text.hsla()))
                .on_click(move |_event, _window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
                })
                .child(
                    svg()
                        .path("icons/copy.svg")
                        .size(px(13.0))
                        .text_color(theme.text_3.hsla()),
                )
        })
        .into_any_element()
}

#[derive(Clone, Default)]
struct InlineStyle {
    bold: bool,
    italic: bool,
    strikethrough: bool,
    link: Option<String>,
}

enum InlineUnitKind {
    Text(String),
    Code(String),
    FileReference { path: String, label: String },
    Break,
}

struct InlineUnit {
    kind: InlineUnitKind,
    style: InlineStyle,
    start: usize,
    end: usize,
    space_after: bool,
}

#[derive(Default)]
struct InlineBuilder {
    units: Vec<InlineUnit>,
}

impl InlineBuilder {
    fn push_text(&mut self, value: &str, source_start: usize, style: &InlineStyle) {
        let mut run_start = None;
        for (offset, character) in value.char_indices() {
            if character.is_whitespace() {
                if let Some(start) = run_start.take() {
                    self.push_text_run(value, source_start, start, offset, style);
                }
                if let Some(last) = self.units.last_mut()
                    && !matches!(last.kind, InlineUnitKind::Break)
                {
                    last.space_after = true;
                }
            } else if run_start.is_none() {
                run_start = Some(offset);
            }
        }
        if let Some(start) = run_start {
            self.push_text_run(value, source_start, start, value.len(), style);
        }
    }

    fn push_text_run(
        &mut self,
        value: &str,
        source_start: usize,
        start: usize,
        end: usize,
        style: &InlineStyle,
    ) {
        self.units.push(InlineUnit {
            kind: InlineUnitKind::Text(value[start..end].to_owned()),
            style: style.clone(),
            start: source_start + start,
            end: source_start + end,
            space_after: false,
        });
    }

    fn push_atomic(&mut self, kind: InlineUnitKind, start: usize, end: usize, style: &InlineStyle) {
        self.units.push(InlineUnit {
            kind,
            style: style.clone(),
            start,
            end,
            space_after: false,
        });
    }

    fn push_break(&mut self, start: usize) {
        self.units.push(InlineUnit {
            kind: InlineUnitKind::Break,
            style: InlineStyle::default(),
            start,
            end: start,
            space_after: false,
        });
    }
}

fn render_inline_flow(children: &[Node], context: &RenderContext<'_>) -> AnyElement {
    render_inline_flow_sized(children, context, true)
}

fn render_inline_flow_sized(
    children: &[Node],
    context: &RenderContext<'_>,
    full_width: bool,
) -> AnyElement {
    let mut builder = InlineBuilder::default();
    let style = InlineStyle::default();
    collect_inline(children, &style, context, &mut builder);
    let units = builder
        .units
        .into_iter()
        .map(|unit| render_inline_unit(unit, context));
    div()
        .when(full_width, |flow| flow.w_full())
        .when(!full_width, |flow| flow.w_auto())
        .flex()
        .flex_wrap()
        .items_baseline()
        .children(units)
        .into_any_element()
}

fn collect_inline(
    children: &[Node],
    style: &InlineStyle,
    context: &RenderContext<'_>,
    builder: &mut InlineBuilder,
) {
    for node in children {
        match node {
            Node::Text(text) => {
                builder.push_text(&text.value, node_start(node).unwrap_or_default(), style)
            }
            Node::Strong(strong) => {
                let mut nested = style.clone();
                nested.bold = true;
                collect_inline(&strong.children, &nested, context, builder);
            }
            Node::Emphasis(emphasis) => {
                let mut nested = style.clone();
                nested.italic = true;
                collect_inline(&emphasis.children, &nested, context, builder);
            }
            Node::Delete(delete) => {
                let mut nested = style.clone();
                nested.strikethrough = true;
                collect_inline(&delete.children, &nested, context, builder);
            }
            Node::InlineCode(code) => {
                let (start, end) = node_range(node);
                let kind = if is_file_reference(&code.value) {
                    InlineUnitKind::FileReference {
                        path: code.value.clone(),
                        label: code.value.clone(),
                    }
                } else {
                    InlineUnitKind::Code(code.value.clone())
                };
                builder.push_atomic(kind, start, end, style);
            }
            Node::InlineMath(math) => {
                let (start, end) = node_range(node);
                builder.push_atomic(InlineUnitKind::Code(math.value.clone()), start, end, style);
            }
            Node::Link(link) => {
                if let Some(path) = local_file_reference_path(&link.url) {
                    let (start, end) = node_range(node);
                    builder.push_atomic(
                        InlineUnitKind::FileReference {
                            path,
                            label: flatten_inline_text(&link.children),
                        },
                        start,
                        end,
                        style,
                    );
                } else {
                    let mut nested = style.clone();
                    nested.link = Some(link.url.clone());
                    collect_inline(&link.children, &nested, context, builder);
                }
            }
            Node::LinkReference(link) => {
                let mut nested = style.clone();
                nested.link = context.definitions.get(&link.identifier).cloned();
                collect_inline(&link.children, &nested, context, builder);
            }
            Node::Break(_) => builder.push_break(node_start(node).unwrap_or_default()),
            Node::Html(html) => {
                builder.push_text(&html.value, node_start(node).unwrap_or_default(), style)
            }
            Node::FootnoteReference(reference) => builder.push_text(
                &format!("[^{}]", reference.identifier),
                node_start(node).unwrap_or_default(),
                style,
            ),
            _ => {}
        }
    }
}

fn render_inline_unit(unit: InlineUnit, context: &RenderContext<'_>) -> AnyElement {
    if matches!(unit.kind, InlineUnitKind::Break) {
        return div().w_full().h(px(0.0)).flex_none().into_any_element();
    }
    let reveal = context.streaming.then(|| {
        context
            .reveals
            .iter()
            .rev()
            .find(|batch| unit.end > batch.from && unit.start < batch.to)
    });
    let reveal = reveal.flatten().map(|batch| {
        let mut reveal_ordinals = context.reveal_ordinals.borrow_mut();
        let ordinal = reveal_ordinals.entry(batch.generation).or_default();
        let result = (batch.generation, *ordinal);
        *ordinal += 1;
        result
    });
    let element = match unit.kind {
        InlineUnitKind::Text(text) => div().flex_none().child(text),
        InlineUnitKind::Code(code) => div()
            .flex_none()
            .rounded(px(5.0))
            .bg(context.theme.surface_2.hsla())
            .px(px(5.0))
            .py(px(1.0))
            .font_family("Geist Mono")
            .text_size(px(13.5))
            .child(code),
        InlineUnitKind::FileReference { path, label } => {
            render_file_reference(&path, label, context.theme)
        }
        InlineUnitKind::Break => unreachable!(),
    };
    let mut element = element.id(SharedString::from(format!(
        "markdown-token:{}:{}",
        context.id, unit.start
    )));
    if unit.space_after {
        element = element.mr(px(SPACE_WIDTH));
    }
    if unit.style.bold {
        element = element.font_weight(FontWeight::BOLD);
    }
    if unit.style.italic {
        element = element.italic();
    }
    if unit.style.strikethrough {
        element = element.line_through();
    }
    if let Some(url) = unit.style.link {
        element = element
            .underline()
            .cursor_pointer()
            .on_click(move |_event, _window, cx| cx.open_url(&url));
    }
    if let Some((generation, ordinal)) = reveal {
        let delay = STREAM_WORD_STAGGER_MS * ordinal as u64;
        let total = STREAM_WORD_DURATION_MS + delay;
        let animation_id = SharedString::from(format!(
            "markdown-word:{}:{generation}:{}",
            context.id, unit.start
        ));
        element
            .with_animation(
                animation_id,
                Animation::new(Duration::from_millis(total)),
                move |word, delta| {
                    let elapsed = delta * total as f32;
                    let local =
                        ((elapsed - delay as f32) / STREAM_WORD_DURATION_MS as f32).clamp(0.0, 1.0);
                    word.opacity(web_ease_out(local))
                },
            )
            .into_any_element()
    } else {
        element.into_any_element()
    }
}

fn render_file_reference(path: &str, label: String, theme: Theme) -> gpui::Div {
    let spec = file_icon_spec(path);
    let icon = if let Some(path) = spec.icon {
        svg()
            .path(path)
            .size(px(15.0))
            .text_color(theme.file_reference.hsla())
            .into_any_element()
    } else {
        div()
            .size(px(15.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(2.0))
            .border_1()
            .border_color(theme.file_reference.hsla())
            .font_family("Geist")
            .font_weight(FontWeight(680.0))
            .text_size(px(7.2))
            .line_height(relative(1.0))
            .child(spec.label.unwrap_or(""))
            .into_any_element()
    };
    div()
        .flex_none()
        .flex()
        .items_center()
        .whitespace_nowrap()
        .text_color(theme.file_reference.hsla())
        .child(div().relative().top(px(1.2)).mr(px(3.0)).child(icon))
        .child(label)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileIconSpec {
    kind: &'static str,
    icon: Option<&'static str>,
    label: Option<&'static str>,
}

fn file_icon_spec(path: &str) -> FileIconSpec {
    let kind = icon_kind(path).unwrap_or("code");
    match kind {
        "react" => icon_spec(kind, "icons/atom.svg"),
        "typescript" => label_spec(kind, "TS"),
        "javascript" => label_spec(kind, "JS"),
        "style" => icon_spec(kind, "icons/hash.svg"),
        "markup" => icon_spec(kind, "icons/code-xml.svg"),
        "data" => icon_spec(kind, "icons/braces.svg"),
        "markdown" => icon_spec(kind, "icons/pilcrow.svg"),
        "text" => icon_spec(kind, "icons/file-text.svg"),
        "database" => icon_spec(kind, "icons/database.svg"),
        "shell" => icon_spec(kind, "icons/square-terminal.svg"),
        "python" => label_spec(kind, "PY"),
        "rust" => icon_spec(kind, "icons/cog.svg"),
        "ruby" => icon_spec(kind, "icons/gem.svg"),
        "java" => icon_spec(kind, "icons/coffee.svg"),
        "kotlin" => label_spec(kind, "K"),
        "go" => label_spec(kind, "GO"),
        "swift" => icon_spec(kind, "icons/bird.svg"),
        "php" => label_spec(kind, "PHP"),
        "c" => label_spec(kind, "C"),
        "cpp" => label_spec(kind, "C++"),
        "csharp" => label_spec(kind, "C#"),
        "vue" => label_spec(kind, "V"),
        "svelte" => label_spec(kind, "S"),
        "image" => icon_spec(kind, "icons/image.svg"),
        "config" => icon_spec(kind, "icons/settings-2.svg"),
        "docker" => icon_spec(kind, "icons/container.svg"),
        _ => icon_spec("code", "icons/file-code-2.svg"),
    }
}

fn icon_spec(kind: &'static str, icon: &'static str) -> FileIconSpec {
    FileIconSpec {
        kind,
        icon: Some(icon),
        label: None,
    }
}

fn label_spec(kind: &'static str, label: &'static str) -> FileIconSpec {
    FileIconSpec {
        kind,
        icon: None,
        label: Some(label),
    }
}

pub(super) fn is_file_reference(path: &str) -> bool {
    icon_kind(path).is_some()
}

fn icon_kind(path: &str) -> Option<&'static str> {
    let without_position = strip_file_position(path);
    let filename = without_position
        .rsplit(['/', '\\'])
        .next()?
        .to_ascii_lowercase();
    let named = match filename.as_str() {
        "dockerfile" => Some("docker"),
        "makefile" | "procfile" => Some("shell"),
        ".gitignore" | ".gitattributes" => Some("config"),
        "license" => Some("text"),
        "readme" => Some("markdown"),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    let extension = filename
        .rsplit_once('.')
        .map_or("", |(_, extension)| extension);
    match extension {
        "tsx" | "jsx" => Some("react"),
        "ts" | "mts" | "cts" => Some("typescript"),
        "js" | "mjs" | "cjs" => Some("javascript"),
        "css" | "scss" | "sass" | "less" => Some("style"),
        "html" | "htm" | "xml" | "svg" => Some("markup"),
        "json" | "jsonc" | "yaml" | "yml" | "toml" | "csv" => Some("data"),
        "md" | "mdx" | "adoc" => Some("markdown"),
        "txt" => Some("text"),
        "sql" | "prisma" => Some("database"),
        "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd" => Some("shell"),
        "py" => Some("python"),
        "rs" => Some("rust"),
        "rb" => Some("ruby"),
        "java" => Some("java"),
        "kt" | "kts" => Some("kotlin"),
        "go" => Some("go"),
        "swift" => Some("swift"),
        "php" => Some("php"),
        "c" | "h" => Some("c"),
        "cc" | "cpp" | "cxx" | "hpp" => Some("cpp"),
        "cs" => Some("csharp"),
        "vue" => Some("vue"),
        "svelte" => Some("svelte"),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "ico" | "bmp" => Some("image"),
        "ini" | "conf" | "env" => Some("config"),
        "graphql" | "gql" | "dart" | "ex" | "exs" | "lua" => Some("code"),
        _ => None,
    }
}

fn strip_file_position(path: &str) -> &str {
    let bytes = path.as_bytes();
    let mut end = bytes.len();
    for _ in 0..2 {
        let Some(colon) = path[..end].rfind(':') else {
            break;
        };
        if colon + 1 < end
            && path[colon + 1..end]
                .bytes()
                .all(|byte| byte.is_ascii_digit())
        {
            end = colon;
        } else {
            break;
        }
    }
    &path[..end]
}

fn local_file_reference_path(href: &str) -> Option<String> {
    let decoded = percent_decode(href).unwrap_or_else(|| href.to_owned());
    let local = decoded.starts_with('/')
        || decoded.starts_with("\\\\")
        || decoded
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
        || is_windows_absolute_path(&decoded);
    if !local {
        return None;
    }
    let without_anchor = decoded.split(['?', '#']).next().unwrap_or(decoded.as_str());
    is_file_reference(without_anchor).then(|| without_anchor.to_owned())
}

fn is_windows_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

fn percent_decode(value: &str) -> Option<String> {
    String::from_utf8(percent_decode_bytes(value)?).ok()
}

fn percent_decode_bytes(value: &str) -> Option<Vec<u8>> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes.get(index + 1)?;
            let low = *bytes.get(index + 2)?;
            output.push(hex_value(high)? * 16 + hex_value(low)?);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    Some(output)
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn contains_media(nodes: &[Node]) -> bool {
    nodes.iter().any(|node| match node {
        Node::Image(_) | Node::ImageReference(_) => true,
        Node::Strong(node) => contains_media(&node.children),
        Node::Emphasis(node) => contains_media(&node.children),
        Node::Delete(node) => contains_media(&node.children),
        Node::Link(node) => contains_media(&node.children),
        Node::LinkReference(node) => contains_media(&node.children),
        _ => false,
    })
}

fn flatten_inline_text(nodes: &[Node]) -> String {
    let mut output = String::new();
    for node in nodes {
        match node {
            Node::Text(node) => output.push_str(&node.value),
            Node::InlineCode(node) => output.push_str(&node.value),
            Node::Strong(node) => output.push_str(&flatten_inline_text(&node.children)),
            Node::Emphasis(node) => output.push_str(&flatten_inline_text(&node.children)),
            Node::Delete(node) => output.push_str(&flatten_inline_text(&node.children)),
            Node::Break(_) => output.push(' '),
            _ => {}
        }
    }
    output
}

fn collect_definitions(nodes: &[Node]) -> HashMap<String, String> {
    nodes
        .iter()
        .filter_map(|node| match node {
            Node::Definition(definition) => {
                Some((definition.identifier.clone(), definition.url.clone()))
            }
            _ => None,
        })
        .collect()
}

fn node_start(node: &Node) -> Option<usize> {
    node.position().map(|position| position.start.offset)
}

fn node_range(node: &Node) -> (usize, usize) {
    node.position().map_or((0, 0), |position| {
        (position.start.offset, position.end.offset)
    })
}

fn stable_hash(value: &str) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_reference_detection_matches_the_web_component() {
        assert_eq!(icon_kind("Sidebar.tsx"), Some("react"));
        assert_eq!(icon_kind("app.css"), Some("style"));
        assert_eq!(icon_kind("README.md:12:4"), Some("markdown"));
        assert_eq!(icon_kind(r"C:\\work\\main.rs:9"), Some("rust"));
        assert_eq!(icon_kind("Dockerfile"), Some("docker"));
        assert_eq!(icon_kind("pnpm typecheck"), None);
        assert!(is_file_reference("schema.sql"));
        assert!(!is_file_reference("no-extension"));
        assert_eq!(file_icon_spec("view.tsx").kind, "react");
        assert_eq!(file_icon_spec("script.ts").label, Some("TS"));
    }

    #[test]
    fn local_file_links_are_decoded_but_malformed_escapes_are_preserved() {
        assert_eq!(
            local_file_reference_path("/Users/blue/My%20App/Sidebar.tsx:119#L2"),
            Some("/Users/blue/My App/Sidebar.tsx:119".into())
        );
        assert_eq!(
            local_file_reference_path(r"C:\\work\\main.rs:9"),
            Some(r"C:\\work\\main.rs:9".into())
        );
        assert_eq!(
            local_file_reference_path("https://example.com/app.ts"),
            None
        );
        assert_eq!(
            local_file_reference_path("/tmp/bad%2G.rs"),
            Some("/tmp/bad%2G.rs".into())
        );
    }

    #[test]
    fn markdown_images_decode_data_and_resolve_project_paths() {
        let (format, bytes) =
            decode_data_image("data:image/png;base64,iVBORw0KGgo=").expect("valid data image");
        assert_eq!(format, ImageFormat::Png);
        assert_eq!(bytes, b"\x89PNG\r\n\x1a\n");
        assert_eq!(
            markdown_image_path("shots/result%201.png#preview", Some("/work/project")),
            Some(PathBuf::from("/work/project/shots/result 1.png"))
        );
        assert_eq!(markdown_image_path("https://example.com/a.png", None), None);
    }

    #[test]
    fn markdown_image_download_names_match_streamdown_rules() {
        assert_eq!(
            image_download_filename(
                "https://example.com/shots/result.webp?raw=1",
                "ignored",
                Some("png")
            ),
            "result.webp"
        );
        assert_eq!(
            image_download_filename("data:image/svg+xml,%3Csvg%2F%3E", "System diagram", None),
            "System diagram.svg"
        );
        assert_eq!(
            image_download_filename("https://example.com/render", "", Some("jpg")),
            "render.jpg"
        );
    }

    #[test]
    fn stream_motion_uses_the_streamdown_timing_contract() {
        assert_eq!(stream_batch_duration(1), Duration::from_millis(160));
        assert_eq!(stream_batch_duration(4), Duration::from_millis(202));
        assert_eq!(STREAM_WORD_STAGGER_MS, 14);
    }

    #[test]
    fn incomplete_markdown_remains_renderable_while_streaming() {
        let parsed = ::markdown::to_mdast(
            "A **partial reply\n\n```rust\nfn main(",
            &ParseOptions::gfm(),
        );
        assert!(matches!(parsed, Ok(Node::Root(_))));
    }

    #[test]
    fn table_copy_uses_tsv_without_markdown_delimiters() {
        let parsed = ::markdown::to_mdast(
            "| Name | Count |\n| --- | ---: |\n| A | 2 |",
            &ParseOptions::gfm(),
        )
        .expect("GFM table should parse");
        let Node::Root(root) = parsed else {
            panic!("expected a Markdown root");
        };
        let table = root.children.iter().find_map(|node| match node {
            Node::Table(table) => Some(table),
            _ => None,
        });
        let data = table.map(table_data);

        assert_eq!(
            data.as_ref().map(format_table_tsv),
            Some("Name\tCount\nA\t2".into())
        );
    }

    #[test]
    fn table_exports_match_streamdown_escaping() {
        let data = TableData {
            headers: vec!["A|B".into(), "Count".into()],
            rows: vec![
                vec![r"x\y".into(), "2".into()],
                vec!["quoted, \"value\"".into(), "line\nbreak".into()],
            ],
        };

        assert_eq!(
            format_table_markdown(&data),
            "| A\\|B | Count |\n| --- | --- |\n| x\\\\y | 2 |\n| quoted, \"value\" | line\nbreak |"
        );
        assert_eq!(
            format_table_csv(&data),
            "A|B,Count\nx\\y,2\n\"quoted, \"\"value\"\"\",\"line\nbreak\""
        );
        assert_eq!(
            format_table_tsv(&TableData {
                headers: vec!["A\tB".into()],
                rows: vec![vec!["line\r\nbreak".into()]],
            }),
            "A\\tB\nline\\r\\nbreak"
        );
    }
}
