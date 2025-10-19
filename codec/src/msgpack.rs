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

    /// sererialized the msg into buf (with std::io::Writer), and return the size written
    fn encode_into<T: Serialize>(&self, task: &T, buf: &mut Vec<u8>) -> Result<usize, ()> {
        let pre_len = buf.len();
        if let Err(e) = rmp_serde::encode::write_named(buf, task) {
            log::error!("encode error: {:?}", e);
            return Err(());
        } else {
            Ok(buf.len() - pre_len)
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
