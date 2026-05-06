//! Country-code → flag-emoji helper.
//!
//! Football-data.org returns 3-letter FIFA / IOC-style codes (e.g. "ARG",
//! "ESP") in `area.code`, but Unicode flag emojis are derived from ISO
//! 3166-1 alpha-2 codes (e.g. "AR", "ES") via the regional-indicator
//! transform: each ASCII letter `L` maps to the codepoint
//! `0x1F1E6 + (L - 'A')`, and a flag is the two-letter sequence joined
//! together.
//!
//! This module bridges the gap with a small mapping for nations that play
//! World Cup qualifiers, then falls back to:
//!   1. Treating the input as already-ISO-2 (works for league teams like
//!      EPL clubs whose `area.code` is "GB").
//!   2. A neutral white-flag glyph for anything we cannot resolve, so the
//!      `flag_emoji NOT NULL` invariant on `teams` is always satisfied.
//!
//! Bias on completeness vs correctness: it is better to ship a placeholder
//! flag than to fail ingestion, so the fallback is always a valid string.

/// Render a flag emoji for the given upstream country code.
///
/// Accepts both 2-letter ISO 3166-1 alpha-2 codes and 3-letter FIFA / IOC
/// codes. Case-insensitive. Returns the placeholder white flag (🏳️) when the
/// code cannot be resolved — never an empty string.
pub fn flag_emoji(country_code: &str) -> String {
    let trimmed = country_code.trim();
    if trimmed.is_empty() {
        return PLACEHOLDER_FLAG.to_string();
    }

    let upper = trimmed.to_ascii_uppercase();

    // Direct hit on the ISO-2 short path (e.g. "AR", "FR").
    if upper.len() == 2 {
        if let Some(emoji) = iso2_to_emoji(&upper) {
            return emoji;
        }
    }

    // Try to translate FIFA / IOC 3-letter codes via the lookup table.
    if upper.len() == 3 {
        if let Some(iso2) = fifa3_to_iso2(&upper) {
            if let Some(emoji) = iso2_to_emoji(iso2) {
                return emoji;
            }
        }
    }

    PLACEHOLDER_FLAG.to_string()
}

/// Neutral fallback. Schemas require a non-null flag_emoji; we return this
/// instead of an empty string when we cannot resolve the country.
const PLACEHOLDER_FLAG: &str = "\u{1F3F3}\u{FE0F}";

/// Build the regional-indicator pair for a 2-letter ISO code.
///
/// Returns `None` if either character is outside the ASCII A-Z range — this
/// guards against codes like "U-20" or numeric placeholders that some
/// providers ship for reserve teams.
fn iso2_to_emoji(code: &str) -> Option<String> {
    let bytes = code.as_bytes();
    if bytes.len() != 2 {
        return None;
    }
    let mut out = String::with_capacity(8);
    for &b in bytes {
        if !b.is_ascii_uppercase() {
            return None;
        }
        // 0x1F1E6 is REGIONAL INDICATOR SYMBOL LETTER A.
        let codepoint = 0x1F1E6u32 + (b - b'A') as u32;
        let ch = char::from_u32(codepoint)?;
        out.push(ch);
    }
    Some(out)
}

/// Resolve a FIFA / IOC 3-letter code to its ISO 3166-1 alpha-2 equivalent.
///
/// Curated to cover the 48 nations that have realistic odds of qualifying
/// for the 2026 World Cup (UEFA top-15, CONMEBOL all, CONCACAF top-8, AFC
/// top-8, CAF top-9, OFC top-2). When a code is missing, callers fall
/// through to the placeholder flag — extending this table is preferred over
/// silently shipping a wrong flag.
fn fifa3_to_iso2(code: &str) -> Option<&'static str> {
    let pair = match code {
        // CONMEBOL
        "ARG" => "AR",
        "BRA" => "BR",
        "URU" => "UY",
        "COL" => "CO",
        "CHI" => "CL",
        "PAR" => "PY",
        "PER" => "PE",
        "ECU" => "EC",
        "BOL" => "BO",
        "VEN" => "VE",
        // UEFA
        "ESP" => "ES",
        "FRA" => "FR",
        "GER" => "DE",
        "ITA" => "IT",
        "POR" => "PT",
        "ENG" => "GB",
        "NED" => "NL",
        "BEL" => "BE",
        "SUI" => "CH",
        "CRO" => "HR",
        "DEN" => "DK",
        "POL" => "PL",
        "SWE" => "SE",
        "NOR" => "NO",
        "AUT" => "AT",
        "TUR" => "TR",
        "UKR" => "UA",
        "SRB" => "RS",
        "CZE" => "CZ",
        "WAL" => "GB",
        "SCO" => "GB",
        "IRL" => "IE",
        "HUN" => "HU",
        "GRE" => "GR",
        "ROU" => "RO",
        "SVK" => "SK",
        "FIN" => "FI",
        "BIH" => "BA",
        "ALB" => "AL",
        "ISL" => "IS",
        "RUS" => "RU",
        "BUL" => "BG",
        // CONCACAF
        "USA" => "US",
        "MEX" => "MX",
        "CAN" => "CA",
        "CRC" => "CR",
        "PAN" => "PA",
        "JAM" => "JM",
        "HON" => "HN",
        "SLV" => "SV",
        "GUA" => "GT",
        "HAI" => "HT",
        "TRI" => "TT",
        "CUB" => "CU",
        "CPV" => "CV",
        "CUW" => "CW",
        "CUR" => "CW",
        // AFC
        "JPN" => "JP",
        "KOR" => "KR",
        "AUS" => "AU",
        "IRN" => "IR",
        "KSA" => "SA",
        "QAT" => "QA",
        "UAE" => "AE",
        "IRQ" => "IQ",
        "CHN" => "CN",
        "UZB" => "UZ",
        "JOR" => "JO",
        "OMA" => "OM",
        "VIE" => "VN",
        "THA" => "TH",
        "IND" => "IN",
        "INA" => "ID",
        // CAF
        "MAR" => "MA",
        "SEN" => "SN",
        "TUN" => "TN",
        "ALG" => "DZ",
        "EGY" => "EG",
        "NGA" => "NG",
        "GHA" => "GH",
        "CMR" => "CM",
        "CIV" => "CI",
        "RSA" => "ZA",
        "MLI" => "ML",
        "CGO" => "CG",
        "BFA" => "BF",
        "GAB" => "GA",
        "GUI" => "GN",
        "ZAM" => "ZM",
        "ETH" => "ET",
        "BEN" => "BJ",
        "TOG" => "TG",
        "ZIM" => "ZW",
        "KEN" => "KE",
        "COD" => "CD",
        // OFC
        "NZL" => "NZ",
        "FIJ" => "FJ",
        "PNG" => "PG",
        // ISO 3166-1 alpha-3 codes the football-data `area.code` field uses
        // when its FIFA / IOC code differs from the FIFA `tla`. Without these
        // entries the team would render with the placeholder flag even
        // though we know exactly which country it is.
        "GBR" => "GB",
        "DEU" => "DE",
        "PRT" => "PT",
        "NLD" => "NL",
        "CHE" => "CH",
        "DNK" => "DK",
        "GRC" => "GR",
        "URY" => "UY",
        "PRY" => "PY",
        "HRV" => "HR",
        "HTI" => "HT",
        "COG" => "CG",
        "TTO" => "TT",
        // Curaçao — the upstream still ships the legacy Netherlands Antilles
        // code "ANT" instead of "CW".
        "ANT" => "CW",
        _ => return None,
    };
    Some(pair)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_iso2_directly() {
        assert_eq!(flag_emoji("AR"), "\u{1F1E6}\u{1F1F7}");
        assert_eq!(flag_emoji("es"), "\u{1F1EA}\u{1F1F8}");
    }

    #[test]
    fn maps_fifa3_via_iso2() {
        assert_eq!(flag_emoji("ARG"), "\u{1F1E6}\u{1F1F7}");
        assert_eq!(flag_emoji("ESP"), "\u{1F1EA}\u{1F1F8}");
        assert_eq!(flag_emoji("USA"), "\u{1F1FA}\u{1F1F8}");
    }

    #[test]
    fn unknown_codes_fall_back_to_placeholder() {
        assert_eq!(flag_emoji("???"), PLACEHOLDER_FLAG);
        assert_eq!(flag_emoji(""), PLACEHOLDER_FLAG);
        assert_eq!(flag_emoji("U20"), PLACEHOLDER_FLAG);
    }
}
