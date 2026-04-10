//! Core type definitions for variants, sizes, and other enums

use std::fmt;

/// Component variant types (following shadcn/ui patterns)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Variant {
    /// Default/Primary variant
    Default,
    /// Secondary variant
    Secondary,
    /// Destructive/Danger variant
    Destructive,
    /// Outline variant
    Outline,
    /// Ghost variant (transparent background)
    Ghost,
    /// Link variant (styled as link)
    Link,
}

impl fmt::Display for Variant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::Secondary => write!(f, "secondary"),
            Self::Destructive => write!(f, "destructive"),
            Self::Outline => write!(f, "outline"),
            Self::Ghost => write!(f, "ghost"),
            Self::Link => write!(f, "link"),
        }
    }
}

/// Component size types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Size {
    /// Extra small
    XS,
    /// Small
    SM,
    /// Medium (default)
    MD,
    /// Large
    LG,
    /// Extra large
    XL,
    /// Icon size (square, typically for icon-only buttons)
    Icon,
    /// Small icon size
    IconSM,
    /// Large icon size
    IconLG,
}

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::XS => write!(f, "xs"),
            Self::SM => write!(f, "sm"),
            Self::MD => write!(f, "md"),
            Self::LG => write!(f, "lg"),
            Self::XL => write!(f, "xl"),
            Self::Icon => write!(f, "icon"),
            Self::IconSM => write!(f, "icon-sm"),
            Self::IconLG => write!(f, "icon-lg"),
        }
    }
}

/// Border radius types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Radius {
    /// No border radius
    None,
    /// Small border radius
    SM,
    /// Medium border radius (default)
    MD,
    /// Large border radius
    LG,
    /// Extra large border radius
    XL,
    /// Full border radius (circular)
    Full,
}

impl fmt::Display for Radius {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::SM => write!(f, "sm"),
            Self::MD => write!(f, "md"),
            Self::LG => write!(f, "lg"),
            Self::XL => write!(f, "xl"),
            Self::Full => write!(f, "full"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variant_display() {
        assert_eq!(Variant::Default.to_string(), "default");
        assert_eq!(Variant::Destructive.to_string(), "destructive");
    }

    #[test]
    fn test_size_display() {
        assert_eq!(Size::SM.to_string(), "sm");
        assert_eq!(Size::Icon.to_string(), "icon");
    }

    #[test]
    fn test_radius_display() {
        assert_eq!(Radius::MD.to_string(), "md");
        assert_eq!(Radius::Full.to_string(), "full");
    }
}
