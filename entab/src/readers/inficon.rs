use alloc::vec::Vec;
use alloc::{format, vec};
use core::marker::Copy;

use crate::parsers::{extract, extract_opt, Endian, FromSlice, SeekPattern};
use crate::record::StateMetadata;
use crate::EtError;
use crate::{impl_reader, impl_record};

/// The current state of the Inficon reader
#[derive(Clone, Debug, Default)]
pub struct InficonState {
    mz_segments: Vec<Vec<f64>>,
    data_left: usize,
    cur_time: f64,
    cur_mz: f64,
    cur_intensity: f64,
    cur_segment: usize,
    mzs_left: usize,
}

impl<'r> StateMetadata<'r> for InficonState {}

impl<'r> FromSlice<'r> for InficonState {
    type State = (Vec<Vec<f64>>, usize);

    fn parse(
        rb: &[u8],
        eof: bool,
        consumed: &mut usize,
        (mz_segments, data_left): &mut Self::State,
    ) -> Result<bool, EtError> {
        // probably not super robust, but it works? this appears at the end of
        // the "instrument collection steps" section and it appears to be
        // a constant distance before the "list of mzs" section
        let con = &mut 0;

        if extract_opt::<SeekPattern>(rb, eof, con, b"\xFF\xFF\xFF\xFF\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\xF6\xFF\xFF\xFF\x00\x00\x00\x00")?.is_none() {
            return Err("Could not find m/z header list".into());
        }
        let _ = extract::<&[u8]>(rb, con, 148)?;
        let n_segments = extract::<u32>(rb, con, Endian::Little)? as usize;
        if n_segments > 10000 {
            return Err("Inficon file has too many segments".into());
        }
        // now read all of the collection segments
        *mz_segments = vec![Vec::new(); n_segments];
        for segment in mz_segments.iter_mut() {
            // first 4 bytes appear to be an name/identifier? not sure what
            // the rest is.
            let _ = extract::<&[u8]>(rb, con, 96)?;
            let n_mzs = extract::<u32>(rb, con, Endian::Little)?;
            for _ in 0..n_mzs {
                let start_mz = extract::<u32>(rb, con, Endian::Little)?;
                let end_mz = extract::<u32>(rb, con, Endian::Little)?;
                if start_mz >= end_mz || end_mz >= 4e9 as u32 {
                    // only malformed data should hit this
                    return Err("m/z range is too big or invalid".into());
                }
                // then dwell time (u32; microseconds) and three more u32s
                let _ = extract::<&[u8]>(rb, con, 16)?;
                let i_type = extract::<u32>(rb, con, Endian::Little)?;
                let _ = extract::<&[u8]>(rb, con, 4)?;
                if i_type == 0 {
                    // this is a SIM
                    segment.push(f64::from(start_mz) / 100.);
                } else {
                    // i_type = 1 appears to be "full scan mode"
                    let mut mz = start_mz;
                    while mz < end_mz + 1 {
                        segment.push(f64::from(mz) / 100.);
                        mz += 100;
                    }
                }
            }
        }
        if extract_opt::<SeekPattern>(rb, eof, con, b"\xFF\xFF\xFF\xFFHapsGPIR")?.is_none() {
            return Err("Could not find start of scan data".into());
        }
        // seek to right before the "HapsScan" section because the section
        // length is encoded in the four bytes before the header for that
        let _ = extract::<&[u8]>(rb, con, 180)?;
        let data_length = u64::from(extract::<u32>(rb, con, Endian::Little)?);
        let _ = extract::<&[u8]>(rb, con, 8)?;
        if extract::<&[u8]>(rb, con, 8)? != b"HapsScan" {
            return Err("Data header was malformed".into());
        }
        let _ = extract::<&[u8]>(rb, con, 56)?;
        *data_left = data_length as usize;
        *consumed += *con;
        Ok(true)
    }

    fn get(&mut self, _rb: &[u8], (mz_segments, data_left): &Self::State) -> Result<(), EtError> {
        self.mz_segments = mz_segments.clone();
        self.data_left = *data_left;
        Ok(())
    }
}

/// A single record from an Inficon Hapsite file.
#[derive(Clone, Copy, Debug, Default)]
pub struct InficonRecord {
    time: f64,
    mz: f64,
    intensity: f64,
}

impl_record!(InficonRecord: time, mz, intensity);

impl<'r> FromSlice<'r> for InficonRecord {
    type State = &'r mut InficonState;

    fn parse(
        rb: &[u8],
        _eof: bool,
        consumed: &mut usize,
        state: &mut Self::State,
    ) -> Result<bool, EtError> {
        if state.data_left > 0 {
            return Ok(false);
        }
        let con = &mut 0;
        let mut mzs_left = state.mzs_left;
        if mzs_left == 0 {
            // the first u32 is the number of the record (i.e. from 1 to r_scans)
            let _ = extract::<u32>(rb, con, Endian::Little)?;
            state.cur_time = f64::from(extract::<i32>(rb, con, Endian::Little)?) / 60000.;
            // next value always seems to be 1
            let _ = extract::<u16>(rb, con, Endian::Little)?;
            let n_mzs = usize::from(extract::<u16>(rb, con, Endian::Little)?);
            // next value always seems to be 0xFFFF
            let _ = extract::<u16>(rb, con, Endian::Little)?;
            // the segment is only contained in the top nibble? the bottom is
            // F (e.g. values seem to be 0x0F, 0x1F, 0x2F...)
            state.cur_segment = usize::from(extract::<u16>(rb, con, Endian::Little)? >> 4);
            if state.cur_segment >= state.mz_segments.len() {
                return Err(
                    format!("Invalid segment number ({}) specified", state.cur_segment).into(),
                );
            }
            if n_mzs != state.mz_segments[state.cur_segment].len() {
                return Err(format!(
                    "Number of intensities ({}) doesn't match number of mzs ({})",
                    n_mzs,
                    state.mz_segments[state.cur_segment].len()
                )
                .into());
            }
            mzs_left = n_mzs;
        }
        state.cur_intensity = f64::from(extract::<f32>(rb, con, Endian::Little)?);
        let cur_mz_segment = &state.mz_segments[state.cur_segment];
        state.cur_mz = cur_mz_segment[cur_mz_segment.len() - state.mzs_left];
        state.mzs_left = mzs_left - 1;
        state.data_left = state.data_left.saturating_sub(*con);
        *consumed += *con;
        Ok(true)
    }

    fn get(&mut self, _rb: &[u8], state: &Self::State) -> Result<(), EtError> {
        self.time = state.cur_time;
        self.mz = state.cur_mz;
        self.intensity = state.cur_intensity;
        Ok(())
    }
}

impl_reader!(
    /// A Reader for Inficon Hapsite data.
    ///
    /// This reader is currently untested on CI until we can find some test data
    /// that can be publicly distributed.
    InficonReader,
    InficonRecord,
    InficonState,
    (Vec<Vec<f64>>, usize)
);

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn bad_inficon_fuzzes() -> Result<(), EtError> {
        let data = [
            4, 3, 2, 1, 83, 80, 65, 72, 66, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 246, 255, 255, 255, 0, 0,
            0, 0, 14, 14, 14, 14, 14, 14, 14, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            248, 10, 10, 10, 10, 35, 4, 0, 0, 0, 0, 0, 0, 10, 10, 10, 10, 10, 62, 10, 10, 26, 0, 0,
            0, 42, 42, 4, 0, 0, 0, 0, 0, 0, 10, 10, 10, 10, 10, 62, 10, 10, 10, 0, 0, 0, 0, 0, 0,
            0, 16, 42, 42, 42, 10, 62, 10, 10, 26, 0, 0, 0, 42, 42, 4, 0, 0, 0, 0, 0, 0, 10, 10,
            10, 10, 10, 62, 10, 10, 10, 0, 0, 0, 0, 0, 0, 0, 16, 42, 42, 42,
        ];
        assert!(InficonReader::new(&data[..], (Vec::new(), 0usize)).is_err());

        let data = [
            4, 3, 2, 1, 83, 80, 65, 72, 4, 1, 10, 255, 255, 255, 0, 3, 197, 65, 77, 1, 62, 1, 0, 0,
            255, 255, 255, 255, 255, 255, 62, 10, 10, 10, 10, 62, 10, 10, 10, 8, 10, 62, 10, 10,
            62, 10, 10, 10, 9, 10, 62, 10, 10, 62, 10, 10, 62, 26, 10, 10, 10, 45, 10, 59, 9, 0,
            255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 246, 255, 255, 255, 0, 0, 0, 0, 71, 71, 71, 71, 71, 38,
            200, 62, 10, 255, 255, 255, 255, 169, 77, 86, 139, 139, 116, 116, 116, 116, 116, 246,
            245, 245, 240, 255, 255, 241, 0, 0, 0, 0, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10,
            10, 10, 62, 10, 227, 205, 10, 10, 62, 10, 0, 62, 10, 10, 1, 0, 62, 10, 10, 34, 0, 0, 0,
            0, 0, 0, 0, 10, 10, 10, 10, 8, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10,
            10, 10, 245, 10, 10, 10, 10, 240, 10, 62, 10, 10, 10, 42, 10, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 134, 134, 14,
            62, 10, 10, 62, 59, 42, 10, 10, 10, 62, 0, 13, 10, 10, 227, 10, 10, 62, 0, 13, 10, 10,
            227, 59, 10, 10, 0, 10, 10, 62, 41, 0, 13, 10, 10, 10, 227, 10, 10, 62, 0, 13, 10, 10,
            10, 62, 10, 10, 8, 10, 62, 10, 10, 10, 10, 10, 62, 10, 10, 10, 62, 10, 10, 10, 10, 62,
            10, 10, 10, 9, 10, 62, 10, 10, 255, 255, 255, 175, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 10, 10, 10, 9, 10, 62, 45, 10, 59, 9, 0,
        ];
        assert!(InficonReader::new(&data[..], (Vec::new(), 0usize)).is_err());

        let data = [
            4, 3, 2, 1, 83, 80, 65, 72, 66, 65, 77, 1, 62, 1, 230, 255, 255, 251, 254, 254, 254,
            254, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 0, 10, 62, 10, 59, 10, 10,
            10, 10, 10, 10, 10, 10, 10, 10, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255,
            255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 246, 255, 255, 255, 0, 0, 0, 0, 10, 10, 102, 13, 10, 35, 24, 10, 62, 13,
            10, 13, 227, 5, 62, 10, 227, 134, 134, 10, 62, 10, 10, 62, 42, 10, 10, 10, 62, 0, 13,
            10, 10, 227, 10, 10, 62, 0, 13, 10, 10, 227, 59, 10, 10, 250, 255, 10, 62, 41, 0, 13,
            10, 10, 227, 43, 10, 10, 10, 10, 10, 10, 47, 59, 10, 10, 62, 0, 13, 10, 10, 227, 10,
            10, 227, 59, 10, 10, 0, 10, 10, 10, 10, 26, 10, 10, 41, 0, 13, 10, 10, 227, 59, 10, 10,
            10, 10, 10, 14, 10, 255, 255, 255, 255, 176, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 175, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 245, 240, 255, 255, 255, 255, 255, 169, 77, 86, 139, 139, 116, 35,
            116, 116, 116, 246, 245, 245, 240, 250, 255, 10, 62, 41, 0, 13, 10, 10, 227, 43, 10,
            10, 10, 10, 10, 10, 47, 59, 10, 10, 4, 3, 2, 1, 83, 80, 181, 181, 181, 181, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255,
            255, 255, 255, 255, 58, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 122, 255, 255, 255,
            255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 246, 255, 255, 255, 0, 0, 0, 0, 59, 10, 10, 10, 10, 10, 14, 10, 255, 10,
            10, 10, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 116, 116, 246, 245, 245, 240,
        ];
        assert!(InficonReader::new(&data[..], (Vec::new(), 0usize)).is_err());

        let data = [
            4, 3, 2, 1, 83, 80, 65, 72, 66, 168, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 10, 26, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            246, 255, 255, 255, 0, 0, 0, 0, 10, 10, 102, 13, 10, 35, 24, 10, 62, 13, 10, 13, 227,
            5, 62, 10, 227, 134, 134, 10, 62, 10, 10, 62, 42, 10, 10, 10, 62, 0, 13, 10, 10, 227,
            10, 10, 62, 0, 13, 10, 10, 227, 59, 10, 10, 250, 255, 10, 62, 41, 0, 13, 10, 10, 227,
            43, 10, 10, 10, 10, 10, 10, 47, 59, 10, 10, 62, 0, 13, 10, 10, 227, 10, 10, 227, 59,
            10, 10, 0, 10, 10, 10, 10, 26, 10, 10, 41, 0, 13, 10, 10, 227, 59, 10, 10, 10, 10, 10,
            14, 10, 255, 10, 10, 10, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 181, 181, 181, 181, 181,
            0, 0, 0, 0, 0, 0, 0, 83, 55, 159, 159, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 227, 43, 10, 10, 10, 10, 10, 10, 47, 59, 10, 10, 10, 10, 62, 42, 10,
            10, 10, 62, 0, 13, 10, 10, 227, 10, 10, 62, 0, 13, 10, 10, 227, 59, 10, 10, 250, 255,
            10, 62, 41, 0, 13, 10, 10, 227, 43, 10, 10, 10, 10, 0, 10, 10, 10, 10, 26, 10, 10, 41,
            0, 13, 10, 10, 227, 59, 10, 10, 10, 10, 10, 14, 10, 255, 10, 10, 10, 10, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 245, 240, 255, 255, 255, 255, 255, 169, 77, 86, 139, 139, 116, 35,
            116, 116, 116, 246, 245, 245, 240, 10, 10, 10, 10, 14, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 245, 240, 255, 255, 255, 255, 255, 169, 77, 86, 139, 139, 116, 35, 116, 246, 245,
            245, 240,
        ];
        assert!(InficonReader::new(&data[..], (Vec::new(), 0usize)).is_err());

        Ok(())
    }

    #[test]
    fn slow_inficon_fuzzes() -> Result<(), EtError> {
        let test_data = [
            4, 3, 2, 1, 83, 80, 65, 72, 66, 65, 77, 1, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 140, 130, 127, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 246, 255, 255,
            255, 0, 0, 0, 0, 10, 10, 102, 13, 10, 35, 24, 10, 62, 13, 10, 13, 227, 5, 62, 10, 227,
            134, 134, 10, 62, 10, 10, 62, 42, 10, 10, 10, 62, 0, 13, 10, 10, 227, 10, 10, 62, 0,
            13, 10, 10, 227, 59, 10, 10, 250, 255, 10, 62, 41, 0, 13, 10, 10, 227, 43, 10, 10, 10,
            10, 10, 10, 47, 59, 10, 10, 62, 0, 13, 10, 10, 227, 10, 10, 227, 59, 10, 10, 0, 10, 10,
            10, 10, 26, 10, 10, 41, 0, 13, 10, 10, 227, 59, 10, 10, 10, 10, 10, 14, 10, 255, 10,
            10, 10, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 181, 181, 181, 181, 181, 0, 0, 0, 0, 0, 0,
            0, 83, 51, 159, 159, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 175, 255, 255, 255, 10, 10, 62, 0,
            13, 10, 10, 220, 227, 10, 10, 62, 0, 13, 10, 10, 227, 10, 10, 62, 10, 59, 10, 10, 10,
            10, 10, 10, 10, 10, 10, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0,
            15, 230, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 227, 59, 10, 10,
            250, 255, 10, 62, 41, 0, 13, 10, 10, 39, 212, 245, 245, 10, 10, 10, 10, 47, 59, 10, 10,
            4, 3, 2, 1, 83, 80, 65, 72, 66, 65, 77, 1, 62, 1, 0, 0, 0, 6, 2, 254, 254, 254, 168,
            168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168,
            168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 1,
            0, 0, 0, 0, 0, 3, 70, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168, 168,
            240, 255, 255, 255, 255, 255, 169, 77, 86, 139, 139, 116, 35, 116, 116, 116, 246, 245,
            245, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237,
            237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 39, 237, 237, 237, 237,
            237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237,
            237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237,
            237, 237, 237, 237, 237, 237, 240,
        ];
        assert!(InficonReader::new(&test_data[..], (Vec::new(), 0usize)).is_err());
        Ok(())
    }
}
