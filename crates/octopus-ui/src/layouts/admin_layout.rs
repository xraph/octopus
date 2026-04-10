//! Admin dashboard layout with sidebar

use crate::core::Node;
use crate::primitives::VStack;

/// Admin layout with sidebar navigation
pub struct AdminLayout {
    title: String,
    nav_items: Vec<NavItem>,
    content: Node,
}

/// Navigation item
pub struct NavItem {
    pub name: String,
    pub path: String,
    pub icon: Option<String>,
    pub active: bool,
}

impl NavItem {
    pub fn new(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            icon: None,
            active: false,
        }
    }

    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
}

impl AdminLayout {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            nav_items: Vec::new(),
            content: Node::empty(),
        }
    }

    pub fn nav_item(mut self, item: NavItem) -> Self {
        self.nav_items.push(item);
        self
    }

    pub fn content(mut self, content: Node) -> Self {
        self.content = content;
        self
    }

    pub fn build(self) -> Node {
        Node::element("div")
            .attr("class", "flex h-full")
            .child(self.sidebar())
            .child(self.main_content())
    }

    fn sidebar(&self) -> Node {
        Node::element("aside")
            .attr("class", "hidden w-64 flex-col border-r bg-muted/40 md:flex")
            .child(
                Node::element("div")
                    .attr("class", "flex h-16 items-center border-b px-6")
                    .child(
                        Node::element("h1")
                            .attr("class", "text-xl font-bold")
                            .child(Node::text("Octopus UI")),
                    ),
            )
            .child({
                let mut nav = Node::element("nav").attr("class", "flex-1 space-y-1 p-4");
                for item in &self.nav_items {
                    nav = nav.child(self.nav_link(item));
                }
                nav
            })
    }

    fn nav_link(&self, item: &NavItem) -> Node {
        let class = if item.active {
            "flex items-center gap-3 rounded-lg bg-muted px-3 py-2 text-primary"
        } else {
            "flex items-center gap-3 rounded-lg px-3 py-2 text-muted-foreground hover:text-primary"
        };

        let mut link = Node::element("a")
            .attr("href", &item.path)
            .attr("class", class);

        if let Some(icon) = &item.icon {
            link = link.child(Node::raw(icon));
        }

        link.child(Node::text(&item.name))
    }

    fn main_content(&self) -> Node {
        Node::element("main")
            .attr("class", "flex-1 overflow-y-auto")
            .child(
                Node::element("div")
                    .attr("class", "border-b")
                    .child(
                        Node::element("div")
                            .attr("class", "flex h-16 items-center px-6")
                            .child(
                                Node::element("h2")
                                    .attr("class", "text-2xl font-bold")
                                    .child(Node::text(&self.title)),
                            )
                            .child(
                                Node::element("div")
                                    .attr("class", "ml-auto flex items-center gap-4")
                                    .child(
                                        Node::element("span")
                                            .attr("class", "text-sm text-muted-foreground")
                                            .child(Node::text("Built with octopus-ui")),
                                    ),
                            ),
                    ),
            )
            .child(
                Node::element("div")
                    .attr("class", "p-6 space-y-6")
                    .child(self.content.clone()),
            )
    }
}

/// Helper to create an admin layout
pub fn admin_layout(title: impl Into<String>) -> AdminLayout {
    AdminLayout::new(title)
}

/// Common navigation icons
pub mod icons {
    pub const HOME: &str = r#"<svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6" /></svg>"#;
    pub const ROUTES: &str = r#"<svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" /></svg>"#;
    pub const HEALTH: &str = r#"<svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" /></svg>"#;
    pub const PLUGINS: &str = r#"<svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4" /></svg>"#;
}


