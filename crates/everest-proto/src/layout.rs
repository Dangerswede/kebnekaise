use std::fmt;

/// Physical key arrangement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum LayoutShape {
    Ansi,
    Iso,
}

/// Keyboard layout as reported by the firmware (`11 12`, reply byte 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Layout {
    Us,
    Uk,
    German,
    French,
    Italian,
    Nordic,
    Portuguese,
    Spanish,
    Hebrew,
    Korean,
}

impl Layout {
    /// Base Camp's mapping. Unknown codes fall back to US, as Base Camp does.
    pub fn from_code(code: u8) -> Self {
        match code {
            3 => Layout::German,
            4 => Layout::Uk,
            5 => Layout::French,
            8 => Layout::Italian,
            11 => Layout::Nordic,
            12 => Layout::Portuguese,
            13 => Layout::Hebrew,
            15 => Layout::Spanish,
            22 => Layout::Korean,
            _ => Layout::Us,
        }
    }

    /// True when the code is one Base Camp names explicitly (17 is US).
    pub fn is_known_code(code: u8) -> bool {
        matches!(code, 3 | 4 | 5 | 8 | 11 | 12 | 13 | 15 | 17 | 22)
    }

    pub fn shape(self) -> LayoutShape {
        match self {
            Layout::Us | Layout::Hebrew | Layout::Korean => LayoutShape::Ansi,
            _ => LayoutShape::Iso,
        }
    }

    /// Whether any source has verified this layout on hardware.
    pub fn verified(self) -> bool {
        matches!(self, Layout::Uk)
    }
}

impl fmt::Display for Layout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Layout::Us => "US",
            Layout::Uk => "UK",
            Layout::German => "German",
            Layout::French => "French",
            Layout::Italian => "Italian",
            Layout::Nordic => "Nordic",
            Layout::Portuguese => "Portuguese",
            Layout::Spanish => "Spanish",
            Layout::Hebrew => "Hebrew",
            Layout::Korean => "Korean",
        };
        f.write_str(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        assert_eq!(Layout::from_code(11), Layout::Nordic);
        assert_eq!(Layout::from_code(4).shape(), LayoutShape::Iso);
        assert_eq!(Layout::from_code(17), Layout::Us);
        assert_eq!(Layout::from_code(99), Layout::Us);
        assert!(!Layout::is_known_code(99));
        assert_eq!(Layout::Korean.shape(), LayoutShape::Ansi);
    }
}
