/// Readers for formats generated by newer versions of the GC/LC control
/// software Chemstation
pub mod chemstation_new;
// TODO: finish and reenable this
// /// Readers for instrument telemetry data generated by Chemstation
// pub mod chemstation_reg;
/// Readers for formats generated by the GC/LC control software Chemstation
pub mod chemstation;

pub use chemstation::{
    ChemstationFidReader, ChemstationFidRecord, ChemstationMsReader, ChemstationMsRecord,
    ChemstationMwdReader, ChemstationMwdRecord,
};
pub use chemstation_new::{ChemstationUvReader, ChemstationUvRecord};

use crate::error::EtError;
use crate::parsers::{extract, Endian, Skip};

/// Read the header chunk for an Agilent file
pub(crate) fn read_agilent_header(rb: &[u8], ms_format: bool) -> Result<usize, EtError> {
    if rb.len() < 268 {
        return Err(EtError::from("Agilent header too short").incomplete());
    }

    // figure out how big the header should be and then get it
    let raw_header_size = extract::<u32>(&rb[264..268], &mut 0, Endian::Big)? as usize;
    if raw_header_size == 0 {
        return Err("Invalid header length of 0".into());
    }
    let mut header_size = 2 * (raw_header_size - 1);
    if !ms_format {
        header_size *= 256;
    }
    if header_size < 512 {
        return Err("Header length too short".into());
    } else if header_size > 20_000 {
        return Err("Header length too long".into());
    }
    let con = &mut 0;
    let _ = extract::<Skip>(rb, con, header_size)?;
    Ok(*con)
}
