use super::AmlError;

pub fn parse_pkg_length(data: &[u8]) -> Result<(usize, usize), AmlError> {
    let lead_byte = data[0];
    let count_bytes: usize = (lead_byte >> 6) as usize;

    if count_bytes == 0 {
        return Ok(((lead_byte & 0x3F) as usize, 1 as usize));
    }

    let upper_two = (lead_byte >> 4) & 0x03;
    if upper_two != 0 {
        return Err(AmlError::AmlParseError("Invalid package length"));
    }

    let mut current_byte = 0;
    let mut pkg_len: usize = (lead_byte & 0x0F) as usize;

    while current_byte < count_bytes {
        pkg_len += (data[1 + current_byte] as u32 * 16 * (256 as u32).pow(current_byte as u32)) as usize;
        current_byte += 1;
    }

    Ok((pkg_len, count_bytes + 1))
}
