pub(crate) fn round(cents: u32) -> u32 {
    cents + crate::cart::line::count()
}
