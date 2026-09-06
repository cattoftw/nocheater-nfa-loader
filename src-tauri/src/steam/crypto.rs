use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};

// Steam formats the CRC32 key as hex with leading zeros stripped and a trailing "1".
pub(crate) fn compute_crc32(data: &str) -> String {
    let crc32_value = crc32fast::hash(data.as_bytes());
    let hex = format!("{crc32_value:08x}");
    let trimmed = hex.trim_start_matches('0');
    if trimmed.is_empty() {
        "01".to_string()
    } else {
        format!("{trimmed}1")
    }
}

fn obfuscate_description() -> Vec<u16> {
    let byte_string =
        b"B\x00O\x00b\x00f\x00u\x00s\x00c\x00a\x00t\x00e\x00B\x00u\x00f\x00f\x00e\x00r\x00\x00\x00";
    let description = String::from_utf8_lossy(byte_string);
    description.encode_utf16().chain(Some(0)).collect()
}

fn local_free(ptr: *mut u8) {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(hmem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }
    unsafe {
        LocalFree(ptr as *mut std::ffi::c_void);
    }
}

// DPAPI (CryptProtectData) with the account name as entropy and Steam's "BObfuscateBuffer" description blob.
pub(crate) fn steam_encrypt(token: &str, account_name: &str) -> Result<String, String> {
    let data_to_encrypt = token.as_bytes();
    let account_name_bytes = account_name.as_bytes();

    let data_in = CRYPT_INTEGER_BLOB {
        cbData: data_to_encrypt.len() as u32,
        pbData: data_to_encrypt.as_ptr() as *mut u8,
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: account_name_bytes.len() as u32,
        pbData: account_name_bytes.as_ptr() as *mut u8,
    };

    let description_wide = obfuscate_description();
    let description_pcwstr = windows::core::PCWSTR(description_wide.as_ptr());
    let mut data_out = CRYPT_INTEGER_BLOB::default();

    unsafe {
        let success = CryptProtectData(
            &data_in,
            description_pcwstr,
            Some(&entropy),
            None,
            None,
            0x11,
            &mut data_out,
        );
        if success.is_err() {
            return Err("CryptProtectData failed".to_string());
        }

        let encrypted_slice = std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize);
        let hex_string = encrypted_slice
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();

        local_free(data_out.pbData);
        Ok(hex_string)
    }
}

/// Decrypt a ConnectCache hex blob with the account name as DPAPI entropy.
pub(crate) fn steam_decrypt(hex: &str, account_name: &str) -> Result<String, String> {
    let hex = hex.trim();
    if hex.is_empty() || hex.len() % 2 != 0 {
        return Err("Invalid ConnectCache blob".into());
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let mut chars = hex.chars();
    while let Some(a) = chars.next() {
        let b = chars.next().ok_or_else(|| "Invalid ConnectCache blob".to_string())?;
        let byte = u8::from_str_radix(&format!("{a}{b}"), 16)
            .map_err(|_| "Invalid ConnectCache hex".to_string())?;
        bytes.push(byte);
    }

    let account_name_bytes = account_name.as_bytes();
    let data_in = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_mut_ptr(),
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: account_name_bytes.len() as u32,
        pbData: account_name_bytes.as_ptr() as *mut u8,
    };
    let mut data_out = CRYPT_INTEGER_BLOB::default();

    unsafe {
        // Prefer the same flag set we encrypt with; fall back to UI_FORBIDDEN only.
        for flags in [0x11u32, 0x1u32, 0u32] {
            let success = CryptUnprotectData(
                &data_in,
                None,
                Some(&entropy),
                None,
                None,
                flags,
                &mut data_out,
            );
            if success.is_ok() && !data_out.pbData.is_null() && data_out.cbData > 0 {
                let plain =
                    std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize).to_vec();
                local_free(data_out.pbData);
                return String::from_utf8(plain)
                    .map_err(|_| "Decrypted token was not valid UTF-8".to_string());
            }
        }
    }

    Err("Could not decrypt Steam ConnectCache token".into())
}
