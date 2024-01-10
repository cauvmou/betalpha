const BASE36_DIGITS: [char; 36] = ['0','1','2','3','4','5','6','7','8','9','a','b','c','d','e','f','g','h','i','j','k','l','m','n','o','p','q','r','s','t','u','v','w','x','y','z'];

pub fn lossy_u64_from_base36(input: &str) -> u64 {
    input.chars().filter_map(|c| BASE36_DIGITS.iter().position(|&x| x == c)).rev().enumerate().map(|(index, digit)| (digit as u64) * (36u64.pow(index as u32))).sum()
}

pub fn base36_from_u64(mut input: u64) -> String {
    let mut chars = Vec::new();
    while input > 0 {
        chars.push(BASE36_DIGITS[(input % 36) as usize]);
        input /= 36;
    }
    String::from_iter(chars.iter().rev())
}

#[test]
fn test_lossy_u64_from_base36() {
    assert_eq!(lossy_u64_from_base36("ya"), 1234);
    assert_eq!(lossy_u64_from_base36("7cik2"), 12341234);
}

#[test]
fn test_base36_from_u64() {
    assert_eq!(base36_from_u64(1234), "ya");
    assert_eq!(base36_from_u64(12341234), "7cik2");
}