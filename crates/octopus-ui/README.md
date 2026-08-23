# Octopus UI

A type-safe UI component library for Rust web applications, inspired by [forgeui](https://github.com/xraph/forgeui) and [gomponents](https://www.gomponents.com).

## Features

- **Type-Safe Components**: Compile-time guarantees for component construction
- **Gomponents-Style API**: Pure Rust components that render to HTML
- **CVA System**: Class Variance Authority for flexible variant management
- **35+ Components**: Production-ready UI components
- **Layout Primitives**: Container, Stack, Grid, Flex, and more
- **Icon System**: SVG icon generation with customization
- **Alpine.js/HTMX Helpers**: Attribute builders for interactivity
- **Theme System**: CSS variables and color token management

## Philosophy

Like [gomponents](https://www.gomponents.com), octopus-ui believes that:

1. **Components are just functions** - No special syntax or DSL to learn
2. **It's all just Rust** - Your editor helps with auto-completion, type-safety, and formatting
3. **Composition over inheritance** - Build complex UIs from simple, reusable components
4. **HTML is the output** - Components render to standard HTML5

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
octopus-ui = { path = "../octopus-ui" }
```

## Quick Start

### Basic Component

```rust
use octopus_ui::core::{Node, Render};

fn greeting(name: &str) -> Node {
    Node::element("div")
        .attr("class", "greeting")
        .child(
            Node::element("h1")
                .child(Node::text(format!("Hello, {}!", name)))
        )
}

fn main() {
    let html = greeting("World").render_to_string();
    println!("{}", html);
    // Output: <div class="greeting"><h1>Hello, World!</h1></div>
}
```

### Using Builder Components

```rust
use octopus_ui::prelude::*;

fn main() {
    let button = Button::with_text("Click me")
        .variant(Variant::Primary)
        .size(Size::Large)
        .render();

    println!("{}", button.render_to_string());
}
```

### Gomponents-Style Usage

```rust
use octopus_ui::core::{Node, Render, map, if_node, Classes};

struct NavLink {
    name: String,
    path: String,
}

fn navbar(logged_in: bool, links: &[NavLink], current_path: &str) -> Node {
    Node::element("nav")
        .attr("class", "navbar")
        .child(
            Node::element("ol")
                .child(
                    map(links, |link| {
                        navbar_item(&link.name, &link.path, link.path == current_path)
                    })
                )
                .child(
                    if_node(logged_in, navbar_item("Log out", "/logout", false))
                )
        )
}

fn navbar_item(name: &str, path: &str, active: bool) -> Node {
    let classes = Classes::new()
        .add("navbar-item", true)
        .add("active", active)
        .build();

    Node::element("li")
        .attr("class", &classes)
        .child(
            Node::element("a")
                .attr("href", path)
                .child(Node::text(name))
        )
}
```

## Core Concepts

### Node

The fundamental building block. Every component returns a `Node`:

```rust
// Element node
Node::element("div")
    .attr("class", "container")
    .child(Node::text("Content"))

// Text node
Node::text("Hello")

// Raw HTML (unescaped)
Node::raw("<strong>Bold</strong>")

// Group (renders children without wrapper)
Node::group(vec![
    Node::text("First"),
    Node::text("Second"),
])

// Empty node (renders nothing)
Node::empty()
```

### Conditional Rendering

```rust
use octopus_ui::core::{if_node, if_lazy};

// Simple condition
if_node(user.is_admin(), Node::text("Admin Panel"))

// Lazy evaluation (avoids expensive computation)
if_lazy(user.is_admin(), || {
    expensive_admin_panel()
})
```

### Mapping Collections

```rust
use octopus_ui::core::map;

let users = vec!["Alice", "Bob", "Charlie"];

Node::element("ul")
    .child(
        map(&users, |user| {
            Node::element("li").child(Node::text(user))
        })
    )
```

### Conditional Classes

```rust
use octopus_ui::core::Classes;

let classes = Classes::new()
    .add("btn", true)
    .add("btn-primary", is_primary)
    .add("btn-disabled", !enabled)
    .build();

Node::element("button").attr("class", &classes)
```

## Components

### Button

```rust
use octopus_ui::components::Button;
use octopus_ui::core::{Variant, Size};

// Basic button
Button::with_text("Click me").render()

// Styled button
Button::with_text("Save")
    .variant(Variant::Primary)
    .size(Size::Large)
    .disabled(false)
    .render()

// Loading button
Button::with_text("Loading...")
    .loading(true)
    .render()

// Icon button
IconButton::new()
    .child(Icon::check().render())
    .render()
```

### Card

```rust
use octopus_ui::components::card::*;

Card::new()
    .child(
        CardHeader::new()
            .child(CardTitle::new("Title").render())
            .child(CardDescription::new("Description").render())
            .render()
    )
    .child(
        CardContent::new()
            .child(Node::text("Content here"))
            .render()
    )
    .child(
        CardFooter::new()
            .child(Button::with_text("Action").render())
            .render()
    )
    .render()
```

### Badge

```rust
use octopus_ui::components::Badge;

Badge::new("New")
    .variant(Variant::Destructive)
    .render()
```

## Layout Primitives

### Stack (Vertical/Horizontal)

```rust
use octopus_ui::primitives::{VStack, HStack};

// Vertical stack
VStack::new()
    .gap("4")
    .child(Node::text("Item 1"))
    .child(Node::text("Item 2"))
    .render()

// Horizontal stack
HStack::new()
    .gap("2")
    .child(Button::with_text("Save").render())
    .child(Button::with_text("Cancel").render())
    .render()
```

### Grid

```rust
use octopus_ui::primitives::Grid;

Grid::new()
    .cols(3)
    .gap("4")
    .child(Node::text("Item 1"))
    .child(Node::text("Item 2"))
    .child(Node::text("Item 3"))
    .render()
```

### Container

```rust
use octopus_ui::primitives::Container;

Container::new()
    .child(Node::text("Centered content"))
    .render()
```

## Helpers

### Alpine.js

```rust
use octopus_ui::helpers::alpine::Alpine;

Node::element("div")
    .attr(Alpine::x_data("{count: 0}").0, Alpine::x_data("{count: 0}").1)
    .child(
        Node::element("button")
            .attr(Alpine::at_click("count++").0, Alpine::at_click("count++").1)
            .child(Node::text("Increment"))
    )
```

### HTMX

```rust
use octopus_ui::helpers::htmx::Htmx;

Node::element("button")
    .attr(Htmx::hx_get("/api/data").0, Htmx::hx_get("/api/data").1)
    .attr(Htmx::hx_target("#results").0, Htmx::hx_target("#results").1)
    .child(Node::text("Load Data"))
```

## Theme System

```rust
use octopus_ui::theme::Theme;

let theme = Theme::default_theme();
let css = theme.to_css_variables();

// Inject into your HTML head
Node::element("style").child(Node::raw(css))
```

## Examples

See the `examples/` directory:

- `gomponents_style.rs` - Gomponents-style component patterns
- Run with: `cargo run --example gomponents_style`

## Comparison with Gomponents

| Feature | Gomponents (Go) | Octopus UI (Rust) |
|---------|----------------|-------------------|
| Component Model | Functions returning `Node` | Functions returning `Node` |
| Type Safety | Go's type system | Rust's type system |
| Conditional Rendering | `If()` helper | `if_node()`, `if_lazy()` |
| Mapping | `Map()` helper | `map()`, `map_indexed()` |
| Classes | `Classes{}` map | `Classes` builder |
| Raw HTML | `Raw()` | `Node::raw()` |
| Fragments | `Group()` | `Node::group()`, `fragment()` |

## Integration with Askama

While octopus-ui can generate HTML strings directly, it integrates seamlessly with Askama templates:

```rust
// In your Rust code
use octopus_ui::components::Button;

let button_html = Button::with_text("Click me")
    .variant(Variant::Primary)
    .render_to_string();

// Pass to Askama template
#[derive(Template)]
#[template(path = "page.html")]
struct PageTemplate {
    button_html: String,
}
```

```html
<!-- In your Askama template -->
<div class="container">
    {{ button_html|safe }}
</div>
```

## Why Octopus UI?

1. **Pure Rust** - No DSL to learn, just Rust functions
2. **Type-Safe** - Catch errors at compile time
3. **Composable** - Build complex UIs from simple components
4. **Familiar** - If you know HTML and Rust, you know octopus-ui
5. **Flexible** - Use standalone or integrate with existing templates
6. **Production-Ready** - Based on shadcn/ui design patterns

## License

MIT OR Apache-2.0

## Credits

Inspired by:
- [gomponents](https://www.gomponents.com) - HTML components in pure Go
- [forgeui](https://github.com/xraph/forgeui) - SSR-first UI framework for Go
- [shadcn/ui](https://ui.shadcn.com) - Design patterns and component styling
