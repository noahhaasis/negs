//! Presentation projections matching macOS ImageSize and the documented IO policy.
pub fn project(
    source: (i32, i32),
    crop: Option<[f64; 4]>,
    target: Option<u32>,
) -> ((u32, u32), (u32, u32)) {
    let (w, h) = (source.0.max(1) as f64, source.1.max(1) as f64);
    let extent = crop.unwrap_or([0., 0., 1., 1.]);
    let scale = target
        .map(|t| t as f64 / (w * extent[2]).max(h * extent[3]))
        .unwrap_or(1.);
    let working = (
        (w * scale).round().max(1.) as u32,
        (h * scale).round().max(1.) as u32,
    );
    let photo = (
        (working.0 as f64 * extent[2]).round().max(1.) as u32,
        (working.1 as f64 * extent[3]).round().max(1.) as u32,
    );
    (working, photo)
}
pub fn validate(text: &str, source: (i32, i32), crop: Option<[f64; 4]>) -> Result<u32, String> {
    let value = text
        .trim()
        .replace(',', "")
        .parse::<u32>()
        .ok()
        .filter(|n| (16..=65_535).contains(n))
        .ok_or("Enter a whole number from 16 to 65,535 pixels.")?;
    let (working, _) = project(source, crop, Some(value));
    let pixels = working.0 as u64 * working.1 as u64;
    if source.0 > 0
        && source.1 > 0
        && pixels > 128_000_000
        && pixels > source.0 as u64 * source.1 as u64
    {
        return Err(format!(
            "Needs {} × {} px, over the 128 MP working limit. Choose a smaller longest side or a larger crop.",
            working.0, working.1
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    #[test]
    fn crop_extent_changes_working_size_and_validation() {
        assert_eq!(
            super::project((6000, 4000), Some([0.1, 0.2, 0.5, 0.5]), Some(3000)),
            ((6000, 4000), (3000, 2000))
        );
        assert!(super::validate("65,535", (6000, 4000), None).is_err());
        assert_eq!(super::validate("2,048", (6000, 4000), None), Ok(2048));
        assert!(super::validate("15", (6000, 4000), None).is_err());
    }
}
