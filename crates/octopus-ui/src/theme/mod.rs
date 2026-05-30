//! Theme system with CSS variables and color tokens

use serde::{Deserialize, Serialize};

/// Theme configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    /// Theme name
    pub name: String,
    /// Color tokens
    pub colors: ColorTokens,
    /// Radius tokens
    pub radius: RadiusTokens,
}

/// Color tokens for theming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorTokens {
    /// Primary color
    pub primary: String,
    /// Primary foreground color
    pub primary_foreground: String,
    /// Secondary color
    pub secondary: String,
    /// Secondary foreground color
    pub secondary_foreground: String,
    /// Destructive color
    pub destructive: String,
    /// Destructive foreground color
    pub destructive_foreground: String,
    /// Muted color
    pub muted: String,
    /// Muted foreground color
    pub muted_foreground: String,
    /// Accent color
    pub accent: String,
    /// Accent foreground color
    pub accent_foreground: String,
    /// Background color
    pub background: String,
    /// Foreground color
    pub foreground: String,
    /// Card background color
    pub card: String,
    /// Card foreground color
    pub card_foreground: String,
    /// Popover background color
    pub popover: String,
    /// Popover foreground color
    pub popover_foreground: String,
    /// Border color
    pub border: String,
    /// Input border color
    pub input: String,
    /// Ring color for focus states
    pub ring: String,
}

/// Border radius tokens
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadiusTokens {
    /// Default radius
    pub default: String,
    /// Small radius
    pub sm: String,
    /// Medium radius
    pub md: String,
    /// Large radius
    pub lg: String,
    /// Extra large radius
    pub xl: String,
}

impl Theme {
    /// Create the default theme
    #[must_use]
    pub fn default_theme() -> Self {
        Self {
            name: "default".to_string(),
            colors: ColorTokens {
                primary: "222.2 47.4% 11.2%".to_string(),
                primary_foreground: "210 40% 98%".to_string(),
                secondary: "210 40% 96.1%".to_string(),
                secondary_foreground: "222.2 47.4% 11.2%".to_string(),
                destructive: "0 84.2% 60.2%".to_string(),
                destructive_foreground: "210 40% 98%".to_string(),
                muted: "210 40% 96.1%".to_string(),
                muted_foreground: "215.4 16.3% 46.9%".to_string(),
                accent: "210 40% 96.1%".to_string(),
                accent_foreground: "222.2 47.4% 11.2%".to_string(),
                background: "0 0% 100%".to_string(),
                foreground: "222.2 84% 4.9%".to_string(),
                card: "0 0% 100%".to_string(),
                card_foreground: "222.2 84% 4.9%".to_string(),
                popover: "0 0% 100%".to_string(),
                popover_foreground: "222.2 84% 4.9%".to_string(),
                border: "214.3 31.8% 91.4%".to_string(),
                input: "214.3 31.8% 91.4%".to_string(),
                ring: "222.2 84% 4.9%".to_string(),
            },
            radius: RadiusTokens {
                default: "0.5rem".to_string(),
                sm: "0.375rem".to_string(),
                md: "0.5rem".to_string(),
                lg: "0.75rem".to_string(),
                xl: "1rem".to_string(),
            },
        }
    }

    /// Generate CSS variables from theme
    #[must_use]
    pub fn to_css_variables(&self) -> String {
        format!(
            r":root {{
  --primary: {};
  --primary-foreground: {};
  --secondary: {};
  --secondary-foreground: {};
  --destructive: {};
  --destructive-foreground: {};
  --muted: {};
  --muted-foreground: {};
  --accent: {};
  --accent-foreground: {};
  --background: {};
  --foreground: {};
  --card: {};
  --card-foreground: {};
  --popover: {};
  --popover-foreground: {};
  --border: {};
  --input: {};
  --ring: {};
  --radius: {};
}}",
            self.colors.primary,
            self.colors.primary_foreground,
            self.colors.secondary,
            self.colors.secondary_foreground,
            self.colors.destructive,
            self.colors.destructive_foreground,
            self.colors.muted,
            self.colors.muted_foreground,
            self.colors.accent,
            self.colors.accent_foreground,
            self.colors.background,
            self.colors.foreground,
            self.colors.card,
            self.colors.card_foreground,
            self.colors.popover,
            self.colors.popover_foreground,
            self.colors.border,
            self.colors.input,
            self.colors.ring,
            self.radius.default,
        )
    }
}
