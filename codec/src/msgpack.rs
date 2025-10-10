use crate::Codec;
use serde::{Deserialize, Serialize};

#[derive(Default)]
pub struct MsgpCodec();

impl Codec for MsgpCodec {
    #[inline(always)]
    fn encode<T: Serialize>(&self, task: &T) -> Result<Vec<u8>, ()> {
        match rmp_serde::encode::to_vec_named(task) {
            Ok(buf) => return Ok(buf),
            Err(e) => {
                log::error!("encode error: {:?}", e);
                return Err(());
            }
        }
    }

    #[inline(always)]
    fn decode<'a, T: Deserialize<'a>>(&self, buf: &'a [u8]) -> Result<T, ()> {
        match rmp_serde::decode::from_slice::<T>(buf) {
            Err(e) => {
                log::warn!("decode error: {:?}", e);
                return Err(());
            }
            Ok(s) => return Ok(s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    #[test]
    fn test_msgp() {
        let codec = MsgpCodec::default();
        let encoded = codec.encode(&()).expect("encode");
        println!("encoded () size :{}", encoded.len());
        let _decoded: () = codec.decode(&encoded).expect("decode");
    }
}
