//! Concise controller names and model-number ordering for device/preset menus.
use super::*;
use std::cmp::Ordering;

// Template requirements belong in Details; keep primary choices to device names.
pub(in crate::preferences) fn preset_label(name: &str) -> &str {
    name.split_once(" (")
        .map_or(name, |(device, _)| device)
        .trim()
}

pub(in crate::preferences) fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (lower_a, lower_b) = (a.to_lowercase(), b.to_lowercase());
    let (mut a_bytes, mut b_bytes) = (lower_a.as_bytes(), lower_b.as_bytes());
    while let (Some(&a_first), Some(&b_first)) = (a_bytes.first(), b_bytes.first()) {
        if a_first.is_ascii_digit() && b_first.is_ascii_digit() {
            let a_len = a_bytes.iter().take_while(|n| n.is_ascii_digit()).count();
            let b_len = b_bytes.iter().take_while(|n| n.is_ascii_digit()).count();
            let a_number = &a_bytes[..a_len];
            let b_number = &b_bytes[..b_len];
            let a_start = a_number.iter().position(|&n| n != b'0').unwrap_or(a_len);
            let b_start = b_number.iter().position(|&n| n != b'0').unwrap_or(b_len);
            let order = (a_len - a_start)
                .cmp(&(b_len - b_start))
                .then_with(|| a_number[a_start..].cmp(&b_number[b_start..]));
            if order != Ordering::Equal {
                return order;
            }
            a_bytes = &a_bytes[a_len..];
            b_bytes = &b_bytes[b_len..];
        } else {
            let order = a_first.cmp(&b_first);
            if order != Ordering::Equal {
                return order;
            }
            a_bytes = &a_bytes[1..];
            b_bytes = &b_bytes[1..];
        }
    }
    a_bytes.len().cmp(&b_bytes.len()).then_with(|| a.cmp(b))
}

impl Preferences {
    pub(in crate::preferences) fn controller_name(&self, name: &str) -> String {
        let tidy = name.split_whitespace().collect::<Vec<_>>().join(" ");
        mixless_controllers::builtin_model_name(&tidy)
            .map(|name| preset_label(name).to_owned())
            .unwrap_or(tidy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_numbers_sort_naturally_without_losing_generations_or_suffixes() {
        let mut names = vec![
            "DDJ-1000SRT",
            "ddj-FLX10",
            "DDJ-800",
            "DDJ-FLX2",
            "DDJ-200",
            "DDJ-1000",
            "DDJ-FLX6-GT",
            "DDJ-400",
            "DDJ-FLX6",
        ];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            names,
            [
                "DDJ-200",
                "DDJ-400",
                "DDJ-800",
                "DDJ-1000",
                "DDJ-1000SRT",
                "DDJ-FLX2",
                "DDJ-FLX6",
                "DDJ-FLX6-GT",
                "ddj-FLX10"
            ]
        );
        assert_eq!(
            natural_cmp("Traktor X1 MK2", "Traktor X1 MK3"),
            Ordering::Less
        );
        assert_eq!(
            natural_cmp(
                "Device 999999999999999999999999",
                "Device 1000000000000000000000000"
            ),
            Ordering::Less
        );
    }
}
