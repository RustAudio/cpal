#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum Endian {
    BE,
    LE,
}

pub(super) fn get_endianness() -> Endian {
    let n: i16 = 1;
    match n.to_ne_bytes()[0] {
        1 => Endian::LE,
        0 => Endian::BE,
        _ => unreachable!("unexpected value in byte"),
    }
}
