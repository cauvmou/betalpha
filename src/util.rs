const BASE36_DIGITS: [char; 36] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i',
    'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
];

pub fn lossy_u64_from_base36(input: &str) -> u64 {
    input
        .chars()
        .filter_map(|c| BASE36_DIGITS.iter().position(|&x| x == c))
        .rev()
        .enumerate()
        .map(|(index, digit)| (digit as u64) * (36u64.pow(index as u32)))
        .sum()
}

pub fn base36_from_u64(mut input: u64) -> String {
    if input == 0 {
        return "0".to_string();
    }
    let mut chars = Vec::new();
    while input > 0 {
        chars.push(BASE36_DIGITS[(input % 36) as usize]);
        input /= 36;
    }
    String::from_iter(chars.iter().rev())
}

pub fn base36_from_i32(input: i32) -> String {
    let lead = if input < 0 { "-" } else { "" };
    let out = base36_from_u64(input.unsigned_abs() as u64);
    format!("{lead}{out}")
}

pub fn pack_float_pair(yaw: f32, pitch: f32) -> (i8, i8) {
    let yaw_shortened = ((yaw / 360.) * 255.) % 255.;
    let pitch = (((pitch / 360.) * 255.) % 255.) as i8;

    let mut yaw = yaw_shortened as i8;
    if yaw_shortened < -128. {
        yaw = 127 - (yaw_shortened + 128.).abs() as i8
    }
    if yaw_shortened > 128. {
        yaw = -128 + (yaw_shortened - 128.).abs() as i8
    }
    (yaw, pitch)
}

#[test]
fn test_lossy_u64_from_base36() {
    assert_eq!(lossy_u64_from_base36("ya"), 1234);
    assert_eq!(lossy_u64_from_base36("7cik2"), 12341234);
    assert_eq!(lossy_u64_from_base36("0"), 0);
}

#[test]
fn test_base36_from_u64() {
    assert_eq!(base36_from_u64(1234), "ya");
    assert_eq!(base36_from_u64(12341234), "7cik2");
    assert_eq!(base36_from_u64(0), "0");
}

#[test]
fn test_base36_from_i32() {
    assert_eq!(base36_from_i32(-13), "-d");
    assert_eq!(base36_from_i32(0), "0");
}
