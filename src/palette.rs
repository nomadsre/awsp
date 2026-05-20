use crossterm::style::Color;

pub const FG: Color = Color::Rgb {
    r: 0xd8,
    g: 0xd6,
    b: 0xcf,
};
pub const DIM: Color = Color::Rgb {
    r: 0x6b,
    g: 0x6b,
    b: 0x6b,
};
pub const MUTED: Color = Color::Rgb {
    r: 0x8a,
    g: 0x8a,
    b: 0x8a,
};
pub const ACCENT: Color = Color::Rgb {
    r: 0xe8,
    g: 0xa0,
    b: 0x4a,
};
pub const ACCENT_2: Color = Color::Rgb {
    r: 0x7a,
    g: 0xa7,
    b: 0xd6,
};
pub const GREEN: Color = Color::Rgb {
    r: 0x8e,
    g: 0xc0,
    b: 0x7c,
};
pub const RED: Color = Color::Rgb {
    r: 0xe0,
    g: 0x7b,
    b: 0x7b,
};
pub const MAGENTA: Color = Color::Rgb {
    r: 0xc7,
    g: 0x92,
    b: 0xea,
};
pub const ROW_SELECTED_BG: Color = Color::Rgb {
    r: 0x1a,
    g: 0x1a,
    b: 0x1d,
};

pub fn env_color(env: &str) -> Color {
    match env {
        "prod" => RED,
        "staging" => ACCENT,
        "dev" => ACCENT_2,
        "personal" => MAGENTA,
        "client" => GREEN,
        _ => MUTED,
    }
}
