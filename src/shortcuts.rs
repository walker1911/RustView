//! Functional compatibility bindings for Manage, View, Compare and processing.
//! Text fields own their editing keys. See docs/KEYBOARD.md for the public map.

use egui::{Event, InputState, Key, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open,
    OpenFolder,
    Next,
    Previous,
    First,
    Last,
    ZoomIn,
    ZoomOut,
    Fit,
    ActualSize,
    Fullscreen,
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
    Refresh,
    Exit,
    Close,
    ToggleBrowser,
    CopyImage,
    CopyFile,
    CopyPath,
    Delete,
    DeletePermanently,
    Properties,
    ToggleAnimation,
    LegacyCommand(u16),
}

// Only physical Ctrl, Shift and Alt count; accepting extra modifiers would turn
// next-page or batch-edit shortcuts into unrelated first-version actions.
fn modifier_bits(modifiers: Modifiers) -> u8 {
    u8::from(modifiers.ctrl)
        | (u8::from(modifiers.shift) << 1)
        | (u8::from(modifiers.alt) << 2)
        | (u8::from(modifiers.mac_cmd) << 3)
}

fn key_action(key: Key, modifiers: u8, browser: bool) -> Option<Action> {
    use Action::*;
    match (modifiers, key) {
        (1, Key::O) => Some(Open),
        (1, Key::W) => Some(if browser { Exit } else { ToggleBrowser }),
        (0, Key::Enter) if browser => Some(ToggleBrowser),
        (4, Key::Enter) => Some(Properties),
        (3, Key::F) => Some(if browser {
            LegacyCommand(2075)
        } else {
            Fullscreen
        }),
        (0, Key::F5) => Some(Refresh),
        (0, Key::Delete) => Some(Delete),
        (2 | 3, Key::Delete) => Some(DeletePermanently),
        (1, Key::C | Key::Insert) => Some(if browser { CopyFile } else { CopyImage }),
        (3, Key::C) => Some(CopyImage),
        (2, Key::F5) if browser => Some(Refresh),
        _ if browser => legacy_key_action(key, modifiers, 2),
        (0, Key::PageDown | Key::Space) => Some(Next),
        (0, Key::PageUp | Key::Backspace) => Some(Previous),
        (0, Key::Home) => Some(First),
        (0, Key::End) => Some(Last),
        (0, Key::Insert) => Some(Open),
        (0 | 2, Key::Enter) | (0, Key::Escape) => Some(ToggleBrowser),
        (1, Key::F4) => Some(Close),
        (0, Key::F) => Some(Fullscreen),
        (0, Key::G) => Some(ToggleAnimation),
        (0, Key::E) => Some(LegacyCommand(22067)),
        (1, Key::ArrowLeft) => Some(Previous),
        (1, Key::ArrowRight) => Some(Next),
        (5, Key::ArrowLeft) => Some(RotateLeft),
        (5, Key::ArrowRight) => Some(RotateRight),
        (0 | 2, Key::Plus | Key::Equals) => Some(ZoomIn),
        (0 | 2, Key::Minus) => Some(ZoomOut),
        (0, Key::Slash) => Some(ActualSize),
        _ => legacy_key_action(key, modifiers, 552),
    }
}

fn clipboard_modifiers(input: &InputState, keys: &[Key]) -> (Option<Key>, Modifiers) {
    // egui-winit consumes the clipboard key press. Its key release still carries
    // event-time modifiers, even if Ctrl/Shift are released before this frame ends.
    input
        .events
        .iter()
        .rev()
        .find_map(|event| match event {
            Event::Key {
                key,
                pressed: false,
                modifiers,
                ..
            } if keys.contains(key) => Some((Some(*key), *modifiers)),
            _ => None,
        })
        .unwrap_or((None, input.modifiers))
}

fn copy_action(input: &InputState, browser: bool) -> Option<Action> {
    let (key, modifiers) = clipboard_modifiers(input, &[Key::C, Key::Insert]);
    let bits = modifier_bits(modifiers);
    // Ctrl+Shift+Insert is not the old Ctrl+Shift+C image-copy binding.
    if key == Some(Key::Insert) && bits & 2 != 0 {
        return None;
    }
    match bits {
        0 | 1 => Some(if browser {
            Action::CopyFile
        } else {
            Action::CopyImage
        }),
        2 | 3 => Some(Action::CopyImage),
        _ => None, // Alt/other modifiers must not become a different old command.
    }
}

pub fn action(input: &InputState, browser: bool) -> Option<Action> {
    input.events.iter().find_map(|event| match event {
        Event::Copy => copy_action(input, browser),
        Event::Cut => clipboard_command(input, browser, true),
        Event::Paste(_) => clipboard_command(input, browser, false),
        Event::Key {
            key,
            pressed: true,
            modifiers,
            repeat,
            ..
        } => {
            let action = key_action(*key, modifier_bits(*modifiers), browser)?;
            (!*repeat
                || matches!(
                    action,
                    Action::Next | Action::Previous | Action::ZoomIn | Action::ZoomOut
                ))
            .then_some(action)
        }
        // egui has no Asterisk key. Text also preserves the old character
        // accelerators (+, =, -, _, /) across keyboard layouts.
        Event::Text(text) if !browser && modifier_bits(input.modifiers) & !2 == 0 => {
            match text.as_str() {
                "*" => Some(Action::Fit),
                "+" | "=" => Some(Action::ZoomIn),
                "-" | "_" => Some(Action::ZoomOut),
                "/" => Some(Action::ActualSize),
                _ => text
                    .encode_utf16()
                    .next()
                    .filter(|_| text.chars().count() == 1)
                    .and_then(|key| {
                        lookup_accelerator(552, false, modifier_bits(input.modifiers) & !2, key)
                    })
                    .map(command_action),
            }
        }
        Event::Text(text) if browser && text.chars().count() == 1 => lookup_accelerator(
            2,
            false,
            modifier_bits(input.modifiers) & !2,
            text.encode_utf16().next()?,
        )
        .map(command_action),
        _ => None,
    })
}

// BEGIN_COMPAT_BINDINGS
const ACCELERATORS: &[(u16, bool, u8, u16, u16)] = &[
    (2, true, 4, 0x0049, 12262),
    (2, true, 0, 0x0073, 2194),
    (2, true, 1, 0x0042, 2144),
    (2, false, 0, 0x005B, 11151),
    (2, true, 1, 0x0043, 2104),
    (2, true, 1, 0x002D, 2104),
    (2, true, 3, 0x0043, 2036),
    (2, true, 4, 0x0043, 2033),
    (2, true, 1, 0x0058, 2103),
    (2, true, 0, 0x002E, 2031),
    (2, true, 1, 0x002E, 2031),
    (2, true, 2, 0x002E, 2031),
    (2, true, 3, 0x002E, 2031),
    (2, true, 5, 0x0044, 2412),
    (2, true, 4, 0x0041, 2061),
    (2, true, 3, 0x0049, 2494),
    (2, true, 4, 0x004D, 2034),
    (2, true, 1, 0x0056, 2105),
    (2, true, 2, 0x002D, 2105),
    (2, true, 0, 0x0071, 2032),
    (2, true, 0, 0x0072, 2035),
    (2, true, 1, 0x0041, 2037),
    (2, true, 3, 0x0041, 2038),
    (2, true, 1, 0x0049, 2039),
    (2, true, 3, 0x0044, 2040),
    (2, true, 1, 0x00DC, 10657),
    (2, true, 1, 0x004B, 2436),
    (2, true, 4, 0x004B, 22298),
    (2, true, 5, 0x0058, 2138),
    (2, true, 4, 0x0047, 2028),
    (2, true, 1, 0x0057, 2030),
    (2, true, 1, 0x004E, 2024),
    (2, true, 1, 0x004F, 2102),
    (2, true, 1, 0x0050, 2106),
    (2, true, 3, 0x0045, 2186),
    (2, true, 5, 0x0045, 22490),
    (2, true, 3, 0x004F, 2185),
    (2, true, 4, 0x0027, 2021),
    (2, true, 4, 0x006D, 12172),
    (2, true, 4, 0x006B, 12171),
    (2, true, 0, 0x0070, 2211),
    (2, true, 4, 0x0058, 2384),
    (2, true, 4, 0x0025, 2172),
    (2, true, 0, 0x007B, 2012),
    (2, true, 0, 0x0076, 2014),
    (2, true, 0, 0x0079, 2010),
    (2, true, 0, 0x007A, 2011),
    (2, true, 0, 0x0077, 2009),
    (2, true, 0, 0x0075, 2015),
    (2, true, 0, 0x0078, 2013),
    (2, true, 2, 0x0075, 57681),
    (2, false, 4, 0x002C, 2534),
    (2, false, 4, 0x002E, 2533),
    (2, true, 1, 0x0035, 2119),
    (2, true, 1, 0x0065, 2119),
    (2, true, 1, 0x0034, 2118),
    (2, true, 1, 0x0064, 2118),
    (2, true, 1, 0x0031, 2115),
    (2, true, 1, 0x0061, 2115),
    (2, true, 1, 0x0033, 2117),
    (2, true, 1, 0x0063, 2117),
    (2, true, 1, 0x0032, 2116),
    (2, true, 1, 0x0062, 2116),
    (2, true, 1, 0x0030, 2114),
    (2, true, 1, 0x0060, 2114),
    (2, true, 4, 0x0035, 22328),
    (2, true, 4, 0x0065, 22328),
    (2, true, 4, 0x0034, 22327),
    (2, true, 4, 0x0064, 22327),
    (2, true, 4, 0x0031, 22324),
    (2, true, 4, 0x0061, 22324),
    (2, true, 4, 0x0033, 22326),
    (2, true, 4, 0x0063, 22326),
    (2, true, 4, 0x0032, 22325),
    (2, true, 4, 0x0062, 22325),
    (2, true, 4, 0x0030, 22323),
    (2, true, 4, 0x0060, 22323),
    (2, true, 3, 0x004C, 2437),
    (2, true, 2, 0x0009, 2229),
    (2, true, 3, 0x0009, 2229),
    (2, true, 0, 0x0009, 2235),
    (2, true, 1, 0x0009, 2235),
    (2, true, 0, 0x00DC, 12021),
    (2, true, 4, 0x004C, 12238),
    (2, true, 1, 0x004C, 2046),
    (2, true, 5, 0x0042, 10576),
    (2, true, 1, 0x0054, 2047),
    (2, true, 5, 0x0052, 2528),
    (2, true, 1, 0x0046, 2043),
    (2, true, 1, 0x0044, 10579),
    (2, true, 1, 0x0045, 21902),
    (2, true, 1, 0x0047, 2048),
    (2, true, 4, 0x004F, 2050),
    (2, true, 1, 0x0052, 2045),
    (2, true, 1, 0x004A, 2044),
    (2, true, 1, 0x004D, 2435),
    (2, true, 7, 0x0057, 2178),
    (2, true, 4, 0x0057, 2180),
    (2, true, 5, 0x0057, 21897),
    (2, true, 3, 0x0057, 2179),
    (2, true, 0, 0x0008, 2022),
    (2, true, 3, 0x0032, 2002),
    (2, true, 2, 0x0074, 2398),
    (2, true, 5, 0x004C, 2153),
    (2, true, 3, 0x0031, 2001),
    (2, true, 3, 0x0046, 2075),
    (2, true, 3, 0x0035, 2008),
    (2, true, 6, 0x0046, 2181),
    (2, true, 3, 0x0050, 2006),
    (2, true, 4, 0x000D, 2003),
    (2, true, 0, 0x0074, 2143),
    (2, true, 3, 0x0033, 2007),
    (2, true, 0, 0x006D, 2402),
    (2, true, 0, 0x006B, 2401),
    (2, true, 3, 0x0042, 2151),
    (2, true, 3, 0x0037, 1999),
    (2, true, 3, 0x0054, 2154),
    (552, false, 0, 0x002A, 2253),
    (552, false, 0, 0x002B, 2238),
    (552, false, 4, 0x002C, 2534),
    (552, false, 0, 0x002D, 2239),
    (552, false, 4, 0x002E, 2533),
    (552, false, 0, 0x002F, 2254),
    (552, true, 4, 0x0030, 22323),
    (552, true, 4, 0x0031, 22324),
    (552, true, 4, 0x0032, 22325),
    (552, true, 4, 0x0033, 22326),
    (552, true, 4, 0x0034, 22327),
    (552, true, 4, 0x0035, 22328),
    (552, true, 1, 0x0030, 2114),
    (552, true, 1, 0x0031, 2115),
    (552, true, 3, 0x0031, 2315),
    (552, true, 1, 0x0032, 2116),
    (552, true, 3, 0x0032, 2316),
    (552, true, 1, 0x0033, 2117),
    (552, true, 3, 0x0033, 2317),
    (552, true, 1, 0x0034, 2118),
    (552, true, 3, 0x0034, 2318),
    (552, true, 1, 0x0035, 2119),
    (552, true, 3, 0x0035, 2319),
    (552, true, 3, 0x0036, 2320),
    (552, true, 3, 0x0037, 2321),
    (552, true, 3, 0x0038, 2322),
    (552, true, 3, 0x0039, 2323),
    (552, false, 0, 0x003D, 2238),
    (552, false, 0, 0x005B, 2862),
    (552, false, 4, 0x005C, 2277),
    (552, false, 0, 0x005F, 2239),
    (552, true, 1, 0x0041, 2375),
    (552, true, 3, 0x0041, 2313),
    (552, true, 0, 0x0042, 2247),
    (552, true, 1, 0x0042, 2381),
    (552, true, 5, 0x0042, 10576),
    (552, true, 3, 0x0042, 2247),
    (552, true, 1, 0x0043, 2260),
    (552, true, 4, 0x0043, 2274),
    (552, true, 3, 0x0043, 2260),
    (552, true, 5, 0x0044, 2263),
    (552, true, 1, 0x0044, 10556),
    (552, true, 1, 0x0045, 2356),
    (552, true, 2, 0x0045, 2334),
    (552, true, 5, 0x0045, 22490),
    (552, true, 0, 0x0046, 2242),
    (552, true, 1, 0x0046, 2268),
    (552, true, 3, 0x0046, 2242),
    (552, true, 3, 0x0047, 2308),
    (552, true, 3, 0x0048, 2088),
    (552, true, 1, 0x004A, 2285),
    (552, true, 1, 0x004B, 2436),
    (552, true, 4, 0x004B, 22298),
    (552, true, 0, 0x004C, 2160),
    (552, true, 1, 0x004C, 21851),
    (552, true, 2, 0x004C, 2233),
    (552, true, 6, 0x004C, 1993),
    (552, true, 1, 0x004D, 21853),
    (552, true, 4, 0x004D, 2275),
    (552, true, 1, 0x004F, 2258),
    (552, true, 4, 0x004F, 2230),
    (552, true, 1, 0x0050, 2259),
    (552, true, 3, 0x0050, 2282),
    (552, true, 1, 0x0051, 2279),
    (552, true, 3, 0x0051, 2303),
    (552, true, 1, 0x0052, 21852),
    (552, true, 4, 0x0052, 2262),
    (552, true, 1, 0x0053, 10420),
    (552, true, 3, 0x0053, 2314),
    (552, true, 5, 0x0053, 22294),
    (552, true, 0, 0x0054, 2252),
    (552, true, 1, 0x0054, 2284),
    (552, true, 3, 0x0054, 2252),
    (552, true, 1, 0x0056, 2273),
    (552, true, 3, 0x0056, 2310),
    (552, true, 0, 0x006B, 2238),
    (552, true, 1, 0x006B, 2432),
    (552, true, 0, 0x0008, 2271),
    (552, true, 2, 0x0008, 2250),
    (552, true, 0, 0x002E, 2261),
    (552, true, 1, 0x002E, 2261),
    (552, true, 2, 0x002E, 2261),
    (552, true, 3, 0x002E, 2261),
    (552, true, 0, 0x006F, 2254),
    (552, true, 1, 0x006F, 2277),
    (552, true, 4, 0x0028, 2301),
    (552, true, 0, 0x0023, 2236),
    (552, true, 2, 0x0023, 2251),
    (552, true, 0, 0x001B, 2326),
    (552, true, 2, 0x001B, 2327),
    (552, true, 0, 0x0070, 2241),
    (552, true, 2, 0x0070, 2371),
    (552, true, 0, 0x0071, 2262),
    (552, true, 2, 0x0071, 2372),
    (552, true, 2, 0x0072, 2373),
    (552, true, 1, 0x0073, 2255),
    (552, true, 3, 0x0073, 2256),
    (552, true, 0, 0x0074, 2237),
    (552, true, 0, 0x0024, 2234),
    (552, true, 2, 0x0024, 2248),
    (552, true, 0, 0x002D, 2258),
    (552, true, 1, 0x002D, 2260),
    (552, true, 2, 0x002D, 2273),
    (552, true, 5, 0x0025, 2328),
    (552, true, 0, 0x006A, 2253),
    (552, true, 0, 0x0022, 1992),
    (552, true, 2, 0x0022, 2249),
    (552, true, 1, 0x0060, 2114),
    (552, true, 0, 0x0061, 2236),
    (552, true, 1, 0x0061, 2115),
    (552, true, 1, 0x0062, 2116),
    (552, true, 0, 0x0063, 1992),
    (552, true, 1, 0x0063, 2117),
    (552, true, 1, 0x0064, 2118),
    (552, true, 1, 0x0065, 2119),
    (552, true, 0, 0x0067, 2234),
    (552, true, 0, 0x0069, 1993),
    (552, true, 4, 0x0060, 22323),
    (552, true, 4, 0x0061, 22324),
    (552, true, 4, 0x0062, 22325),
    (552, true, 4, 0x0063, 22326),
    (552, true, 4, 0x0064, 22327),
    (552, true, 4, 0x0065, 22328),
    (552, true, 0, 0x00DC, 12210),
    (552, true, 1, 0x00BD, 2431),
    (552, true, 3, 0x00BD, 2431),
    (552, true, 1, 0x00BB, 2432),
    (552, true, 3, 0x00BB, 2432),
    (552, true, 0, 0x0021, 1993),
    (552, true, 2, 0x0021, 2250),
    (552, true, 0, 0x000D, 21947),
    (552, true, 4, 0x000D, 2351),
    (552, true, 2, 0x000D, 21947),
    (552, true, 4, 0x0027, 2300),
    (552, true, 5, 0x0027, 2329),
    (552, true, 0, 0x0020, 2269),
    (552, true, 2, 0x0020, 2249),
    (552, true, 0, 0x006D, 2239),
    (552, true, 1, 0x006D, 2431),
    (552, true, 0, 0x0009, 2324),
    (552, true, 2, 0x0009, 2324),
    (552, true, 1, 0x0057, 2232),
    (552, true, 4, 0x0057, 2267),
    (552, true, 5, 0x0057, 21897),
    (552, true, 3, 0x0057, 2266),
    (552, true, 7, 0x0057, 2265),
    (552, true, 1, 0x0058, 2281),
    (552, true, 4, 0x0058, 2385),
    (552, true, 5, 0x0058, 2138),
    (552, true, 1, 0x0059, 2312),
    (552, true, 0, 0x005A, 2302),
    (552, true, 2, 0x005A, 2331),
    (552, true, 1, 0x005A, 2311),
    (553, true, 0, 0x0022, 1992),
    (553, true, 0, 0x0021, 1993),
    (553, true, 0, 0x002E, 2261),
    (553, true, 0, 0x0034, 2167),
    (553, true, 0, 0x0064, 2167),
    (553, true, 0, 0x0070, 2241),
    (553, true, 0, 0x004F, 22109),
    (553, true, 0, 0x0031, 2164),
    (553, true, 0, 0x0061, 2164),
    (553, true, 0, 0x004C, 2160),
    (553, true, 0, 0x001B, 2326),
    (553, true, 0, 0x00DC, 2489),
    (553, true, 0, 0x004D, 2490),
    (553, true, 0, 0x0033, 2166),
    (553, true, 0, 0x0063, 2166),
    (553, true, 0, 0x0032, 2165),
    (553, true, 0, 0x0062, 2165),
    (553, true, 0, 0x0055, 22109),
    (553, true, 0, 0x0048, 2088),
    (553, true, 3, 0x0048, 2088),
    (553, true, 0, 0x0050, 2351),
    (553, false, 0, 0x002F, 2254),
    (553, true, 0, 0x006F, 2254),
    (553, false, 0, 0x002A, 2253),
    (553, true, 0, 0x006A, 2253),
    (553, true, 4, 0x0028, 2301),
    (553, false, 0, 0x002B, 2238),
    (553, false, 0, 0x003D, 2238),
    (553, true, 0, 0x006B, 2238),
    (553, true, 1, 0x006F, 2277),
    (553, false, 0, 0x002D, 2239),
    (553, false, 0, 0x005F, 2239),
    (553, true, 0, 0x006D, 2239),
    (553, true, 4, 0x0027, 2300),
    (22118, true, 1, 0x0041, 22119),
];
// END_COMPAT_BINDINGS

fn lookup_accelerator(table: u16, virtual_key: bool, modifiers: u8, key: u16) -> Option<u16> {
    ACCELERATORS
        .iter()
        .find_map(|(scope, virtual_flag, mask, code, command)| {
            (*scope == table && *virtual_flag == virtual_key && *mask == modifiers && *code == key)
                .then_some(*command)
        })
}

fn command_action(command: u16) -> Action {
    use Action::*;
    match command {
        2102 | 2258 => Open,
        1992 | 2269 => Next,
        1993 | 2271 => Previous,
        2234 => First,
        2236 => Last,
        2238 => ZoomIn,
        2239 => ZoomOut,
        2253 => Fit,
        2254 => ActualSize,
        2242 => Fullscreen,
        2328 => RotateLeft,
        2329 => RotateRight,
        2143 | 2237 | 2398 => Refresh,
        2030 => Exit,
        2232 => ToggleBrowser,
        2255 => Close,
        21947 | 2326 => ToggleBrowser,
        2036 | 2260 => CopyImage,
        2104 => CopyFile,
        2031 | 2261 => Delete,
        2003 | 2351 => Properties,
        _ => LegacyCommand(command),
    }
}

pub fn allowed_command(command: u16) -> bool {
    include_str!("../docs/retained-command-audit.csv")
        .lines()
        .skip(1)
        .any(|line| {
            line.split(',').next().and_then(|s| s.parse::<u16>().ok()) == Some(command)
                && !line.contains("排除：")
                && !line.contains("排除功能")
                && !line.contains("未激活")
                && !line.contains("尚不能确定")
                && !matches!(command, 2340 | 2430 | 10554 | 22234 | 2471..=2486)
        })
}
pub fn action_for_command(command: u16) -> Option<Action> {
    allowed_command(command).then(|| command_action(command))
}
pub fn default_bindings() -> Vec<crate::legacy_settings::ShortcutBinding> {
    ACCELERATORS
        .iter()
        .filter(|(table, _, _, _, command)| {
            matches!(table, 2 | 552 | 553) && allowed_command(*command)
        })
        .map(|(table, virtual_key, modifiers, key, command)| {
            crate::legacy_settings::ShortcutBinding {
                table: *table,
                virtual_key: *virtual_key,
                modifiers: *modifiers,
                key: *key,
                command: *command,
            }
        })
        .collect()
}

fn egui_virtual_key(key: Key) -> Option<u16> {
    let name = key.name();
    if name.len() == 1 && name.as_bytes()[0].is_ascii_alphanumeric() {
        return Some(name.as_bytes()[0].to_ascii_uppercase() as u16);
    }
    if let Some(number) = name.strip_prefix('F').and_then(|n| n.parse::<u16>().ok()) {
        if (1..=24).contains(&number) {
            return Some(0x6f + number);
        }
    }
    Some(match key {
        Key::Backspace => 8,
        Key::Tab => 9,
        Key::Enter => 13,
        Key::Escape => 27,
        Key::Space => 32,
        Key::PageUp => 33,
        Key::PageDown => 34,
        Key::End => 35,
        Key::Home => 36,
        Key::ArrowLeft => 37,
        Key::ArrowUp => 38,
        Key::ArrowRight => 39,
        Key::ArrowDown => 40,
        Key::Insert => 45,
        Key::Delete => 46,
        Key::Semicolon => 0xba,
        Key::Equals | Key::Plus => 0xbb,
        Key::Comma => 0xbc,
        Key::Minus => 0xbd,
        Key::Period => 0xbe,
        Key::Slash => 0xbf,
        Key::Backtick => 0xc0,
        Key::OpenBracket => 0xdb,
        Key::Backslash => 0xdc,
        Key::CloseBracket => 0xdd,
        Key::Quote => 0xde,
        _ => return None,
    })
}

fn legacy_key_action(key: Key, modifiers: u8, table: u16) -> Option<Action> {
    lookup_accelerator(table, true, modifiers, egui_virtual_key(key)?).map(command_action)
}

fn clipboard_command(input: &InputState, browser: bool, cut: bool) -> Option<Action> {
    let keys = if cut {
        &[Key::X, Key::Delete][..]
    } else {
        &[Key::V, Key::Insert][..]
    };
    let (key, modifiers) = clipboard_modifiers(input, keys);
    let mut bits = modifier_bits(modifiers);
    if key.is_none() && bits == 2 {
        return None;
    } // Unknown Shift+Delete is not Cut.
    if bits == 0 {
        bits = 1;
    }
    key_action(
        key.unwrap_or(if cut { Key::X } else { Key::V }),
        bits,
        browser,
    )
}

/// Windows input takes priority over egui so FVIRTKEY retains VK_NUMPAD1 vs VK_1.
pub fn native_action(
    events: &[crate::native_keys::NativeKeyEvent],
    browser: bool,
) -> Option<Action> {
    native_action_for_table(events, if browser { 2 } else { 552 })
}

/// Extra accelerator contexts (for example 553) must be explicitly selected by
/// their actual dialog/mode. Online 21800 and unconfirmed media 555 are omitted.
pub fn native_action_for_table(
    events: &[crate::native_keys::NativeKeyEvent],
    table: u16,
) -> Option<Action> {
    use crate::native_keys::NativeKeyEvent;
    events.iter().find_map(|event| {
        let (virtual_key, key, modifiers, repeat) = match event {
            NativeKeyEvent::Key {
                virtual_key,
                pressed: true,
                modifiers,
                repeat,
                ..
            } => (true, *virtual_key, modifier_bits(*modifiers), *repeat),
            NativeKeyEvent::Char {
                utf16,
                modifiers,
                repeat,
            } => (false, *utf16, modifier_bits(*modifiers) & !2, *repeat),
            _ => return None,
        };
        let action = if virtual_key && modifiers == 0 && table == 2 && key == 13 {
            Action::ToggleBrowser // Official runtime default-action binding, absent from RT_ACCELERATOR.
        } else if virtual_key && modifiers == 0 && table == 552 && key == 0x47 {
            Action::ToggleAnimation // New, unoccupied G binding; not the removed Auto Advance shortcut.
        } else if virtual_key && modifiers == 0 && table == 552 && key == 0x45 {
            Action::LegacyCommand(22067) // Documented runtime E binding, absent from the resource table.
        } else if virtual_key && modifiers == 1 && table == 552 && key == 0x25 {
            Action::Previous
        } else if virtual_key && modifiers == 1 && table == 552 && key == 0x27 {
            Action::Next
        } else {
            let command = lookup_accelerator(table, virtual_key, modifiers, key)?;
            if modifiers & 2 != 0 && matches!(command, 2031 | 2261) {
                Action::DeletePermanently
            } else {
                command_action(command)
            }
        };
        (!repeat
            || matches!(
                action,
                Action::Next | Action::Previous | Action::ZoomIn | Action::ZoomOut
            ))
        .then_some(action)
    })
}

// Kept for callers of the first-version API. Retained commands now dispatch
// their exact old command ID instead of being replaced with a pending dialog.
pub fn pending_action(_input: &InputState, _browser: bool) -> Option<&'static str> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_bindings_cannot_expand_the_retained_command_scope() {
        let count = include_str!("../docs/retained-command-audit.csv")
            .lines()
            .skip(1)
            .filter_map(|line| line.split(',').next()?.parse::<u16>().ok())
            .filter(|id| allowed_command(*id))
            .count();
        assert_eq!(count, 206);
        for id in [102, 2000, 2005, 2243, 2171, 2293, 2340, 2430, 10554, 22234] {
            assert!(!allowed_command(id), "{id}");
        }
        assert!(allowed_command(22119));
    }

    #[test]
    fn respects_old_mode_and_modifier_meanings() {
        assert_eq!(
            key_action(Key::E, 0, false),
            Some(Action::LegacyCommand(22067))
        );
        assert_eq!(key_action(Key::ArrowRight, 1, false), Some(Action::Next));
        assert_eq!(key_action(Key::C, 1, true), Some(Action::CopyFile));
        assert_eq!(key_action(Key::C, 1, false), Some(Action::CopyImage));
        assert_eq!(key_action(Key::W, 1, true), Some(Action::Exit));
        assert_eq!(key_action(Key::W, 1, false), Some(Action::ToggleBrowser));
        assert_eq!(
            key_action(Key::Escape, 0, false),
            Some(Action::ToggleBrowser)
        );
        assert_eq!(key_action(Key::F4, 1, false), Some(Action::Close));
        assert_eq!(key_action(Key::S, 1, true), None); // Removed slideshow.
        assert_eq!(
            key_action(Key::F11, 0, true),
            Some(Action::LegacyCommand(2011))
        ); // Old list mode.
        assert_eq!(
            key_action(Key::PageDown, 2, false),
            Some(Action::LegacyCommand(2249))
        ); // Next page, not image.
        assert_eq!(
            key_action(Key::ArrowLeft, 5, false),
            Some(Action::RotateLeft)
        );
        assert_eq!(key_action(Key::ArrowLeft, 0, false), None);
        assert_eq!(
            key_action(Key::Delete, 2, false),
            Some(Action::DeletePermanently)
        );
        assert_eq!(key_action(Key::Num1, 0, false), None); // Do not confuse numpads.
    }

    #[test]
    fn character_fit_binding_is_viewer_only() {
        let mut input = InputState::default();
        input.events.push(Event::Text("*".into()));
        assert_eq!(action(&input, false), Some(Action::Fit));
        assert_eq!(action(&input, true), None);
        input.modifiers.ctrl = true;
        assert_eq!(action(&input, false), None);
    }

    #[test]
    fn removed_slideshow_does_not_become_save_or_a_pending_action() {
        let mut input = InputState::default();
        input.events.push(Event::Key {
            key: Key::S,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        });
        assert_eq!(action(&input, true), None);
        assert_eq!(pending_action(&input, true), None);
        assert_eq!(action(&input, false), Some(Action::LegacyCommand(10420)));
        assert_eq!(pending_action(&input, false), None);
        input.events = vec![Event::Text("普通文本".into())];
        assert_eq!(pending_action(&input, false), None);
    }

    #[test]
    fn repeats_navigation_but_never_delete_exit_or_fullscreen() {
        for (key, modifiers, expected) in [
            (Key::Delete, Modifiers::default(), None),
            (
                Key::W,
                Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
                None,
            ),
            (Key::F, Modifiers::default(), None),
            (Key::PageDown, Modifiers::default(), Some(Action::Next)),
            (Key::PageUp, Modifiers::default(), Some(Action::Previous)),
            (Key::Plus, Modifiers::default(), Some(Action::ZoomIn)),
            (Key::Minus, Modifiers::default(), Some(Action::ZoomOut)),
        ] {
            let mut input = InputState::default();
            input.events.push(Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: true,
                modifiers,
            });
            assert_eq!(action(&input, false), expected, "{key:?}");
        }
    }

    #[test]
    fn backend_copy_uses_context_when_key_press_was_consumed() {
        let mut input = InputState::default();
        input.events.push(Event::Copy);
        assert_eq!(action(&input, true), Some(Action::CopyFile));
        assert_eq!(action(&input, false), Some(Action::CopyImage));
        input.events.push(Event::Key {
            key: Key::Insert,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        });
        assert_eq!(action(&input, true), Some(Action::CopyFile));
    }

    #[test]
    fn backend_shift_copy_prefers_release_modifiers_over_end_of_frame_state() {
        let mut input = InputState::default(); // Ctrl and Shift already released.
        input.events = vec![
            Event::Copy,
            Event::Key {
                key: Key::C,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Modifiers {
                    ctrl: true,
                    shift: true,
                    ..Modifiers::default()
                },
            },
        ];
        assert_eq!(action(&input, true), Some(Action::CopyImage));
        assert_eq!(action(&input, false), Some(Action::CopyImage));
        input.events.pop();
        input.modifiers.shift = true; // No release event: use the frame fallback.
        assert_eq!(action(&input, true), Some(Action::CopyImage));
    }

    #[test]
    fn backend_clipboard_events_do_not_steal_alt_commands() {
        let mut input = InputState::default();
        input.events = vec![
            Event::Copy,
            Event::Key {
                key: Key::C,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Modifiers {
                    ctrl: true,
                    alt: true,
                    ..Modifiers::default()
                },
            },
        ];
        assert_eq!(action(&input, true), None);
        assert_eq!(action(&input, false), None);
        input.events = vec![Event::Cut];
        assert_eq!(action(&input, true), Some(Action::LegacyCommand(2103)));
        input.events = vec![Event::Paste("clipboard text".into())];
        assert_eq!(action(&input, false), Some(Action::LegacyCommand(2273)));
        input.modifiers.alt = true;
        assert_eq!(pending_action(&input, false), None);
    }

    fn native_key(
        virtual_key: u16,
        pressed: bool,
        modifiers: Modifiers,
    ) -> crate::native_keys::NativeKeyEvent {
        crate::native_keys::NativeKeyEvent::Key {
            virtual_key,
            scan_code: 0,
            extended: false,
            pressed,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn native_numpad_is_distinct_even_when_released_in_same_frame() {
        let plain = Modifiers::default();
        assert_eq!(
            native_action(
                &[
                    native_key(0x31, true, plain),
                    native_key(0x31, false, plain)
                ],
                false
            ),
            None
        );
        assert_eq!(
            native_action(
                &[
                    native_key(0x61, true, plain),
                    native_key(0x61, false, plain)
                ],
                false
            ),
            Some(Action::Last)
        );
        let ctrl = Modifiers {
            ctrl: true,
            ..plain
        };
        assert_eq!(
            native_action(&[native_key(0x31, true, ctrl)], false),
            Some(Action::LegacyCommand(2115))
        );
        assert_eq!(
            native_action(&[native_key(0x61, true, ctrl)], false),
            Some(Action::LegacyCommand(2115))
        );
    }

    #[test]
    fn native_excludes_network_audio_and_slideshow_but_keeps_filmstrip() {
        let plain = Modifiers::default();
        assert_eq!(
            native_action(&[native_key(0x76, true, plain)], true),
            Some(Action::LegacyCommand(2014))
        );
        assert_eq!(
            native_action(
                &[native_key(0x53, true, Modifiers { alt: true, ..plain })],
                false
            ),
            None
        );
        assert_eq!(
            native_action(
                &[native_key(
                    0x4d,
                    true,
                    Modifiers {
                        ctrl: true,
                        shift: true,
                        ..plain
                    }
                )],
                false
            ),
            None
        );
        assert_eq!(
            native_action(
                &[crate::native_keys::NativeKeyEvent::Char {
                    utf16: 96,
                    repeat: false,
                    modifiers: plain
                }],
                false
            ),
            None
        );
        assert_eq!(
            native_action(&[native_key(0x47, true, plain)], false),
            Some(Action::ToggleAnimation)
        );
        assert_eq!(
            native_action(&[native_key(13, true, plain)], true),
            Some(Action::ToggleBrowser)
        );
    }
}
