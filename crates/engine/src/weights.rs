//! Reads a safetensors file one tensor at a time.
//!
//! The weights stay in 16-bit floats, on disk and in memory. Half the bytes
//! is half the memory of an app that is always running, and it is also the
//! fast way: producing a token means one pass over the whole vocabulary
//! table, and the time of that pass is the time it takes to pull the table
//! through the processor's caches.
//!
//! Only the tensors the model asks for are read, straight into their final
//! buffer. Nothing is memory mapped, so no page of the file stays in memory
//! beside the model. A file in 32-bit floats (what the Hub has for the small
//! models) is accepted too and narrowed on the way in.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use candle_core::{Device, Tensor};
use half::slice::HalfFloatSliceExt;
use serde::Deserialize;

use crate::failure::{Failure, Result};

/// The file name, for messages.
pub(crate) const FILE: &str = "model.safetensors";

/// A header larger than this is not a Marian model: the real ones are about
/// 30 kB.
const LARGEST_HEADER: u64 = 16 * 1024 * 1024;

pub(crate) struct Weights {
    file: File,
    /// Where the tensor data starts in the file, after the header.
    data_start: u64,
    data_len: u64,
    tensors: HashMap<String, Stored>,
}

#[derive(Deserialize)]
struct Stored {
    dtype: String,
    shape: Vec<usize>,
    data_offsets: (u64, u64),
}

impl Weights {
    pub fn open(path: &Path) -> Result<Self> {
        let damaged = |why: &dyn std::fmt::Display| Failure::damaged(FILE, why);
        let mut file = File::open(path).map_err(|error| damaged(&error))?;
        let file_len = file.metadata().map_err(|error| damaged(&error))?.len();

        let mut prefix = [0u8; 8];
        file.read_exact(&mut prefix).map_err(|error| damaged(&error))?;
        let header_len = u64::from_le_bytes(prefix);
        if header_len > LARGEST_HEADER || header_len + 8 > file_len {
            return Err(damaged(&"it does not start with a safetensors header"));
        }
        let mut header = vec![0u8; header_len as usize];
        file.read_exact(&mut header).map_err(|error| damaged(&error))?;

        // `__metadata__` sits next to the tensors and has another shape.
        let entries: HashMap<String, serde_json::Value> =
            serde_json::from_slice(&header).map_err(|error| damaged(&error))?;
        let mut tensors = HashMap::with_capacity(entries.len());
        for (name, entry) in entries {
            if name != "__metadata__" {
                let stored = serde_json::from_value(entry).map_err(|error| damaged(&error))?;
                tensors.insert(name, stored);
            }
        }
        let data_start = header_len + 8;
        Ok(Self { file, data_start, data_len: file_len - data_start, tensors })
    }

    /// The tensor `name` in 16-bit floats, which must have exactly this
    /// shape.
    pub fn tensor(&mut self, name: &str, shape: &[usize]) -> Result<Tensor> {
        self.first_of(&[name], shape)
    }

    /// Like [`Weights::tensor`] for a tensor that converters store under
    /// different names: the first name that exists is read.
    pub fn first_of(&mut self, names: &[&str], shape: &[usize]) -> Result<Tensor> {
        let missing = || Failure::damaged(FILE, format_args!("the tensor {} is missing", names[0]));
        let (name, stored) =
            names.iter().find_map(|name| Some((*name, self.tensors.get(*name)?))).ok_or_else(missing)?;
        let wrong = |why: &str| Failure::damaged(FILE, format_args!("the tensor {name} {why}"));

        let width: usize = match stored.dtype.as_str() {
            "F32" => 4,
            "F16" => 2,
            _ => return Err(wrong("is neither f32 nor f16")),
        };
        if stored.shape != shape {
            return Err(wrong("has another shape than the configuration says"));
        }
        let count: usize = shape.iter().product();
        let (start, end) = stored.data_offsets;
        if end > self.data_len || end.checked_sub(start) != Some((count * width) as u64) {
            return Err(wrong("does not fit in the file"));
        }

        // The file is read into a buffer of the stored type, viewed as
        // bytes: no copy for 16-bit floats, one narrowing pass for 32-bit.
        let mut halves = vec![half::f16::ZERO; count];
        let mut singles = vec![0f32; if width == 4 { count } else { 0 }];
        let bytes: &mut [u8] = match width {
            4 => bytemuck::cast_slice_mut(&mut singles),
            _ => bytemuck::cast_slice_mut(&mut halves),
        };
        self.file
            .seek(SeekFrom::Start(self.data_start + start))
            .and_then(|_| self.file.read_exact(bytes))
            .map_err(|error| Failure::damaged(FILE, error))?;
        if width == 4 {
            halves.convert_from_f32_slice(&singles);
        }
        Ok(Tensor::from_vec(halves, shape, &Device::Cpu)?)
    }
}

// safetensors stores little-endian numbers, and they are read as they are.
#[cfg(target_endian = "big")]
compile_error!("coco-engine reads model weights on little-endian targets only");

#[cfg(test)]
mod tests {
    use candle_core::DType;

    use super::*;

    /// A safetensors file with a 32-bit and a 16-bit tensor that hold the
    /// same four numbers.
    fn sample_file(dir: &Path) -> std::path::PathBuf {
        let header = r#"{"__metadata__":{"format":"pt"},"full":{"dtype":"F32","shape":[2,2],"data_offsets":[0,16]},"half":{"dtype":"F16","shape":[4],"data_offsets":[16,24]}}"#;
        let numbers = [1.0f32, -2.5, 0.0, 1024.0];
        let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
        bytes.extend_from_slice(header.as_bytes());
        numbers.iter().for_each(|n| bytes.extend_from_slice(&n.to_le_bytes()));
        numbers.iter().for_each(|n| bytes.extend_from_slice(&half::f16::from_f32(*n).to_le_bytes()));
        std::fs::create_dir_all(dir).expect("create the test folder");
        let path = dir.join(FILE);
        std::fs::write(&path, bytes).expect("write the test file");
        path
    }

    fn test_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("coco-weights-{name}-{}", std::process::id()))
    }

    #[test]
    fn both_precisions_come_out_as_the_same_16_bit_values() {
        let dir = test_dir("precisions");
        let mut weights = Weights::open(&sample_file(&dir)).expect("open");
        let full = weights.tensor("full", &[2, 2]).expect("the 32-bit tensor");
        let half = weights.tensor("half", &[4]).expect("the 16-bit tensor");
        assert_eq!((full.dtype(), half.dtype()), (DType::F16, DType::F16));
        let values = |tensor: Tensor| tensor.flatten_all()?.to_dtype(DType::F32)?.to_vec1::<f32>();
        assert_eq!(values(full).expect("values"), [1.0, -2.5, 0.0, 1024.0]);
        assert_eq!(values(half).expect("values"), [1.0, -2.5, 0.0, 1024.0]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_missing_tensor_or_a_wrong_shape_is_an_error_not_a_panic() {
        let dir = test_dir("errors");
        let mut weights = Weights::open(&sample_file(&dir)).expect("open");
        assert!(weights.tensor("absent", &[4]).is_err());
        assert!(weights.tensor("full", &[4]).is_err());
        assert!(weights.first_of(&["absent", "half"], &[4]).is_ok());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_file_that_is_not_safetensors_is_refused() {
        let dir = test_dir("garbage");
        std::fs::create_dir_all(&dir).expect("create the test folder");
        let path = dir.join(FILE);
        std::fs::write(&path, b"this is not a model at all").expect("write");
        assert!(Weights::open(&path).is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
