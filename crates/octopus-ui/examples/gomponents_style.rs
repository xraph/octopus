//! Example showing gomponents-style usage of octopus-ui
//!
//! Run with: cargo run --example gomponents_style

use octopus_ui::core::{fragment, if_node, map, Classes, Node};

/// Navigation link data structure
#[derive(Debug, Clone)]
struct NavLink {
    name: String,
    path: String,
}

/// Navbar component (gomponents-style)
fn navbar(logged_in: bool, links: &[NavLink], current_path: &str) -> Node {
    Node::element("nav")
        .attr("class", "navbar")
        .child(Node::element("ol").children(vec![
            // Map links to navbar items
            map(links, |link| {
                navbar_item(&link.name, &link.path, link.path == current_path)
            }),
            // Conditional logout link
            if_node(logged_in, navbar_item("Log out", "/logout", false)),
        ]))
}

/// Navbar item component
fn navbar_item(name: &str, path: &str, active: bool) -> Node {
    let classes = Classes::new()
        .add("navbar-item", true)
        .add("active", active)
        .add("inactive", !active)
        .build();

    Node::element("li").attr("class", &classes).child(
        Node::element("a")
            .attr("href", path)
            .child(Node::text(name)),
    )
}

/// Page layout component
fn page_layout(title: &str, logged_in: bool, content: Node) -> Node {
    let links = vec![
        NavLink {
            name: "Home".to_string(),
            path: "/".to_string(),
        },
        NavLink {
            name: "About".to_string(),
            path: "/about".to_string(),
        },
        NavLink {
            name: "Contact".to_string(),
            path: "/contact".to_string(),
        },
    ];

    Node::element("html")
        .child(Node::element("head").children(vec![
                Node::element("meta")
                    .attr("charset", "UTF-8")
                    .self_closing(),
                Node::element("title").child(Node::text(title)),
                Node::element("link")
                    .attr("rel", "stylesheet")
                    .attr("href", "/styles.css")
                    .self_closing(),
            ]))
        .child(Node::element("body").children(vec![
                navbar(logged_in, &links, "/"),
                Node::element("main")
                    .attr("class", "container")
                    .child(content),
            ]))
}

/// Home page content
fn home_page(logged_in: bool) -> Node {
    fragment(vec![
        Node::element("h1").child(Node::text("Welcome!")),
        Node::element("p").child(Node::text("This is a gomponents-style example.")),
        if_node(
            logged_in,
            Node::element("p").child(Node::text("You are logged in.")),
        ),
        if_node(
            !logged_in,
            Node::element("p")
                .child(Node::text("Please "))
                .child(
                    Node::element("a")
                        .attr("href", "/login")
                        .child(Node::text("log in")),
                )
                .child(Node::text(".")),
        ),
    ])
}

/// User list component with map
fn user_list(users: &[String]) -> Node {
    Node::element("div")
        .attr("class", "user-list")
        .children(vec![
            Node::element("h2").child(Node::text("Users")),
            Node::element("ul").child(map(users, |user| {
                Node::element("li").child(Node::text(user))
            })),
        ])
}

fn main() {
    // Example 1: Simple navbar
    println!("=== Example 1: Navbar ===");
    let nav = navbar(
        true,
        &[
            NavLink {
                name: "Home".to_string(),
                path: "/".to_string(),
            },
            NavLink {
                name: "About".to_string(),
                path: "/about".to_string(),
            },
        ],
        "/",
    );
    println!("{}", nav.render());
    println!();

    // Example 2: Full page layout
    println!("=== Example 2: Full Page ===");
    let page = page_layout("Home", true, home_page(true));
    println!("{}", page.render());
    println!();

    // Example 3: User list with map
    println!("=== Example 3: User List ===");
    let users = vec![
        "Alice".to_string(),
        "Bob".to_string(),
        "Charlie".to_string(),
    ];
    let list = user_list(&users);
    println!("{}", list.render());
    println!();

    // Example 4: Conditional rendering
    println!("=== Example 4: Conditional Rendering ===");
    println!("Logged in:");
    println!("{}", home_page(true).render());
    println!("\nLogged out:");
    println!("{}", home_page(false).render());
}
