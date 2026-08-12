//! Site navigation.
//!
//! A generated page is self-contained by design, and a single file
//! stays that way. When many pages are published together as a site,
//! each one also needs a way back to the others: another service
//! date, the other direction of a line, the hub.
//!
//! [`PageNav`] is that block. It is optional, so a one-off page keeps
//! exactly the shape it has today, and it holds only relative links,
//! so a site works under any path prefix — which GitHub Pages needs,
//! because a project site lives under `/<repository>/`.

use crate::escape;

/// One link in a navigation group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavLink {
    /// The relative target, for example `../index.html`.
    pub href: String,
    /// The visible text.
    pub label: String,
    /// A longer description for assistive technology, when the label
    /// alone is not enough.
    pub title: Option<String>,
    /// Whether this link names the page that the reader is on.
    pub current: bool,
}

impl NavLink {
    /// Make a link.
    pub fn new(href: impl Into<String>, label: impl Into<String>) -> Self {
        NavLink {
            href: href.into(),
            label: label.into(),
            title: None,
            current: false,
        }
    }

    /// Mark the link as the page that the reader is on.
    pub fn current(mut self, current: bool) -> Self {
        self.current = current;
        self
    }

    /// Add a longer description.
    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// A labelled row of links, for example the service dates.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NavGroup {
    /// The heading of the row.
    pub label: String,
    /// The links.
    pub links: Vec<NavLink>,
}

impl NavGroup {
    /// Make a group.
    pub fn new(label: impl Into<String>, links: Vec<NavLink>) -> Self {
        NavGroup {
            label: label.into(),
            links,
        }
    }
}

/// The navigation block of a page inside a published site.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageNav {
    /// The link back to the hub.
    pub home: Option<NavLink>,
    /// The name of the site, beside the home link.
    pub site_name: Option<String>,
    /// The rows of links.
    pub groups: Vec<NavGroup>,
}

impl PageNav {
    /// Report whether the block would render nothing.
    pub fn is_empty(&self) -> bool {
        self.home.is_none() && self.groups.iter().all(|group| group.links.is_empty())
    }
}

/// Write the navigation block.
///
/// The block is a `<nav>` with an accessible name, and it carries the
/// `no-print` class: a printed timetable needs no links.
pub fn render(out: &mut String, nav: &PageNav) {
    if nav.is_empty() {
        return;
    }
    out.push_str("<nav class=\"site-nav no-print\" aria-label=\"Site\">\n");

    if let Some(home) = &nav.home {
        out.push_str("<a class=\"site-home\" href=\"");
        out.push_str(&escape::attr(&home.href));
        out.push_str("\">");
        out.push_str("<span aria-hidden=\"true\">\u{2190}</span> ");
        out.push_str(&escape::text(&home.label));
        out.push_str("</a>\n");
    }
    if let Some(name) = &nav.site_name {
        out.push_str("<span class=\"site-name\">");
        out.push_str(&escape::text(name));
        out.push_str("</span>\n");
    }

    for group in &nav.groups {
        if group.links.is_empty() {
            continue;
        }
        out.push_str("<div class=\"nav-group\">\n<span class=\"nav-label\">");
        out.push_str(&escape::text(&group.label));
        out.push_str("</span>\n<ul>\n");
        for link in &group.links {
            out.push_str("<li><a");
            if link.current {
                // `aria-current` is what a screen reader announces;
                // the class only styles the chip.
                out.push_str(" class=\"current\" aria-current=\"page\"");
            }
            out.push_str(" href=\"");
            out.push_str(&escape::attr(&link.href));
            out.push('"');
            if let Some(title) = &link.title {
                out.push_str(" title=\"");
                out.push_str(&escape::attr(title));
                out.push('"');
            }
            out.push('>');
            out.push_str(&escape::text(&link.label));
            out.push_str("</a></li>\n");
        }
        out.push_str("</ul>\n</div>\n");
    }
    out.push_str("</nav>\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_block_renders_nothing() {
        let mut out = String::new();
        render(&mut out, &PageNav::default());
        assert!(out.is_empty());

        let mut out = String::new();
        render(
            &mut out,
            &PageNav {
                groups: vec![NavGroup::new("Date", Vec::new())],
                ..Default::default()
            },
        );
        assert!(out.is_empty());
    }

    #[test]
    fn the_current_page_is_announced_and_styled() {
        let mut out = String::new();
        render(
            &mut out,
            &PageNav {
                home: Some(NavLink::new("../index.html", "All stations")),
                site_name: Some("MRTracker".into()),
                groups: vec![NavGroup::new(
                    "Service date",
                    vec![
                        NavLink::new("a.html", "Mon").current(true),
                        NavLink::new("b.html", "Tue"),
                    ],
                )],
            },
        );
        assert!(out.contains("aria-current=\"page\""));
        assert_eq!(out.matches("aria-current").count(), 1);
        assert!(out.contains("href=\"../index.html\""));
        assert!(out.contains("class=\"site-nav no-print\""));
    }

    #[test]
    fn hostile_text_cannot_escape_a_link() {
        let mut out = String::new();
        render(
            &mut out,
            &PageNav {
                home: Some(NavLink::new(
                    "\"><script>alert(1)</script>",
                    "</a><img src=x onerror=alert(2)>",
                )),
                site_name: None,
                groups: vec![NavGroup::new(
                    "<b>x</b>",
                    vec![NavLink::new("y.html", "z").titled("\" onmouseover=\"alert(3)")],
                )],
            },
        );
        // No payload opens a tag of its own.
        assert!(!out.contains("<script"));
        assert!(!out.contains("<img"));
        // And no payload closes an attribute to start a new one: the
        // text survives with its angle brackets and quotes escaped.
        assert!(out.contains("&lt;img src=x onerror=alert(2)&gt;"));
        assert!(out.contains("&quot; onmouseover=&quot;alert(3)"));
        assert!(!out.contains("\" onmouseover=\""));
        assert!(out.contains("&lt;b&gt;x&lt;/b&gt;"));
        // The whole block is still one nav element.
        assert_eq!(out.matches("<nav").count(), 1);
    }
}
