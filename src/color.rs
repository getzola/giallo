use serde::{Deserialize, Serialize};

/// RGBA color with 8-bit components
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// Error type for color parsing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseColorError {
    InvalidFormat,
    InvalidLength,
    InvalidHexDigit,
}

impl std::fmt::Display for ParseColorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseColorError::InvalidFormat => write!(f, "Color must start with #"),
            ParseColorError::InvalidLength => {
                write!(f, "Color must be #RGB, #RRGGBB, or #RRGGBBAA")
            }
            ParseColorError::InvalidHexDigit => write!(f, "Invalid hex digit in color"),
        }
    }
}

impl std::error::Error for ParseColorError {}

/// Convert a single hex digit to its numeric value
const fn const_hex_digit(byte: u8) -> Result<u8, ParseColorError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(ParseColorError::InvalidHexDigit),
    }
}

impl Color {
    /// Parse a hex color string at compile time
    /// Supports #RGB, #RRGGBB, and #RRGGBBAA formats
    pub const fn from_hex(hex: &str) -> Result<Color, ParseColorError> {
        let bytes = hex.as_bytes();

        // Check for # prefix
        if bytes.len() == 0 || bytes[0] != b'#' {
            return Err(ParseColorError::InvalidFormat);
        }

        match bytes.len() {
            4 => {
                // #RGB
                let r = match const_hex_digit(bytes[1]) {
                    Ok(val) => val * 16 + val, // Expand 4-bit to 8-bit
                    Err(e) => return Err(e),
                };
                let g = match const_hex_digit(bytes[2]) {
                    Ok(val) => val * 16 + val,
                    Err(e) => return Err(e),
                };
                let b = match const_hex_digit(bytes[3]) {
                    Ok(val) => val * 16 + val,
                    Err(e) => return Err(e),
                };
                Ok(Color { r, g, b, a: 255 })
            }
            7 => {
                // #RRGGBB
                let r = match const_hex_digit(bytes[1]) {
                    Ok(high) => match const_hex_digit(bytes[2]) {
                        Ok(low) => (high << 4) | low,
                        Err(e) => return Err(e),
                    },
                    Err(e) => return Err(e),
                };
                let g = match const_hex_digit(bytes[3]) {
                    Ok(high) => match const_hex_digit(bytes[4]) {
                        Ok(low) => (high << 4) | low,
                        Err(e) => return Err(e),
                    },
                    Err(e) => return Err(e),
                };
                let b = match const_hex_digit(bytes[5]) {
                    Ok(high) => match const_hex_digit(bytes[6]) {
                        Ok(low) => (high << 4) | low,
                        Err(e) => return Err(e),
                    },
                    Err(e) => return Err(e),
                };
                Ok(Color { r, g, b, a: 255 })
            }
            9 => {
                // #RRGGBBAA
                let r = match const_hex_digit(bytes[1]) {
                    Ok(high) => match const_hex_digit(bytes[2]) {
                        Ok(low) => (high << 4) | low,
                        Err(e) => return Err(e),
                    },
                    Err(e) => return Err(e),
                };
                let g = match const_hex_digit(bytes[3]) {
                    Ok(high) => match const_hex_digit(bytes[4]) {
                        Ok(low) => (high << 4) | low,
                        Err(e) => return Err(e),
                    },
                    Err(e) => return Err(e),
                };
                let b = match const_hex_digit(bytes[5]) {
                    Ok(high) => match const_hex_digit(bytes[6]) {
                        Ok(low) => (high << 4) | low,
                        Err(e) => return Err(e),
                    },
                    Err(e) => return Err(e),
                };
                let a = match const_hex_digit(bytes[7]) {
                    Ok(high) => match const_hex_digit(bytes[8]) {
                        Ok(low) => (high << 4) | low,
                        Err(e) => return Err(e),
                    },
                    Err(e) => return Err(e),
                };
                Ok(Color { r, g, b, a })
            }
            _ => Err(ParseColorError::InvalidLength),
        }
    }

    /// Standard color constants
    pub const WHITE: Color = match Self::from_hex("#ffffff") {
        Ok(color) => color,
        Err(_) => Color {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
    };

    pub const BLACK: Color = match Self::from_hex("#000000") {
        Ok(color) => color,
        Err(_) => Color {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        },
    };

    /// Theme fallback defaults (based on Shiki's proven approach)
    pub const DARK_FG_DEFAULT: Color = match Self::from_hex("#bbbbbb") {
        Ok(color) => color,
        Err(_) => Self::WHITE,
    };

    pub const DARK_BG_DEFAULT: Color = match Self::from_hex("#1e1e1e") {
        Ok(color) => color,
        Err(_) => Self::BLACK,
    };

    pub const LIGHT_FG_DEFAULT: Color = match Self::from_hex("#333333") {
        Ok(color) => color,
        Err(_) => Self::BLACK,
    };

    pub const LIGHT_BG_DEFAULT: Color = match Self::from_hex("#fffffe") {
        Ok(color) => color,
        Err(_) => Self::WHITE,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_const_color_parsing() {
        // Test basic colors
        assert_eq!(
            Color::WHITE,
            Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255
            }
        );
        assert_eq!(
            Color::BLACK,
            Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255
            }
        );

        // Test fallback colors
        assert_eq!(
            Color::DARK_FG_DEFAULT,
            Color {
                r: 187,
                g: 187,
                b: 187,
                a: 255
            }
        );
        assert_eq!(
            Color::DARK_BG_DEFAULT,
            Color {
                r: 30,
                g: 30,
                b: 30,
                a: 255
            }
        );
        assert_eq!(
            Color::LIGHT_FG_DEFAULT,
            Color {
                r: 51,
                g: 51,
                b: 51,
                a: 255
            }
        );
        assert_eq!(
            Color::LIGHT_BG_DEFAULT,
            Color {
                r: 255,
                g: 255,
                b: 254,
                a: 255
            }
        );
    }

    #[test]
    fn test_hex_parsing_formats() {
        // #RGB format
        const RED_RGB: Color = match Color::from_hex("#f00") {
            Ok(color) => color,
            Err(_) => Color::BLACK,
        };
        assert_eq!(
            RED_RGB,
            Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255
            }
        );

        // #RRGGBB format
        const BLUE_RRGGBB: Color = match Color::from_hex("#0000ff") {
            Ok(color) => color,
            Err(_) => Color::BLACK,
        };
        assert_eq!(
            BLUE_RRGGBB,
            Color {
                r: 0,
                g: 0,
                b: 255,
                a: 255
            }
        );

        // #RRGGBBAA format
        const GREEN_ALPHA: Color = match Color::from_hex("#00ff0080") {
            Ok(color) => color,
            Err(_) => Color::BLACK,
        };
        assert_eq!(
            GREEN_ALPHA,
            Color {
                r: 0,
                g: 255,
                b: 0,
                a: 128
            }
        );
    }

    #[test]
    fn test_invalid_hex() {
        const _: () = {
            // These would cause compilation errors with invalid hex:
            // const BAD: Color = Color::from_hex("#gggggg").unwrap(); // Would fail to compile

            // Test error cases at runtime
        };
    }
}
