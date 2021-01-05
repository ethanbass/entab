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

use crate::buffer::ReadBuffer;
use crate::error::EtError;
use crate::parsers::{Endian, FromSlice};

/// Read the header chunk for an Agilent file
pub(crate) fn read_agilent_header<'r>(
    rb: &'r mut ReadBuffer,
    ms_format: bool,
) -> Result<&'r [u8], EtError> {
    rb.reserve(268)?;

    // figure out how big the header should be and then get it
    let raw_header_size = u32::out_of(&rb[264..268], Endian::Big)? as usize;
    if raw_header_size == 0 {
        return Err(EtError::new("Invalid header length of 0", &rb));
    }
    let mut header_size = 2 * (raw_header_size - 1);
    if !ms_format {
        header_size *= 256;
    }
    if header_size < 512 {
        return Err(EtError::new("Header length too short", &rb));
    }
    rb.extract::<&[u8]>(header_size)
}
