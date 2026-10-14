use std::io;

use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The largest frame either side accepts (4 MiB), matching `silod`.
pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

/// Reads one length-prefixed JSON frame. `Ok(None)` is a clean end of stream.
pub async fn read_frame<R, T>(reader: &mut R) -> io::Result<Option<T>>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut header = [0_u8; 4];
    if reader.read(&mut header[..1]).await? == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut header[1..]).await?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame length {length} is outside the allowed range"),
        ));
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).await?;
    serde_json::from_slice(&payload)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Writes one length-prefixed JSON frame.
pub async fn write_frame<W, T>(writer: &mut W, value: &T) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let payload = serde_json::to_vec(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if payload.is_empty() || payload.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "frame length {} is outside the allowed range",
                payload.len()
            ),
        ));
    }
    let header = u32::try_from(payload.len())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
        .to_be_bytes();
    writer.write_all(&header).await?;
    writer.write_all(&payload).await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tokio::io::duplex;

    #[tokio::test]
    async fn round_trips_a_frame_with_a_big_endian_prefix() {
        let (mut writer, mut reader) = duplex(64);
        write_frame(&mut writer, &json!({"id": "x"})).await.unwrap();
        let mut header = [0_u8; 4];
        reader.read_exact(&mut header).await.unwrap();
        assert_eq!(u32::from_be_bytes(header) as usize, b"{\"id\":\"x\"}".len());
        let (mut writer, mut reader) = duplex(64);
        write_frame(&mut writer, &json!({"id": "y"})).await.unwrap();
        let value: Value = read_frame(&mut reader).await.unwrap().unwrap();
        assert_eq!(value["id"], "y");
    }

    #[tokio::test]
    async fn rejects_oversized_zero_length_and_invalid_frames() {
        let (mut writer, mut reader) = duplex(16);
        writer
            .write_all(&(MAX_FRAME_BYTES as u32 + 1).to_be_bytes())
            .await
            .unwrap();
        assert!(read_frame::<_, Value>(&mut reader).await.is_err());
        let (mut writer, mut reader) = duplex(16);
        writer.write_all(&0_u32.to_be_bytes()).await.unwrap();
        assert!(read_frame::<_, Value>(&mut reader).await.is_err());
        let (mut writer, mut reader) = duplex(16);
        writer.write_all(&1_u32.to_be_bytes()).await.unwrap();
        writer.write_all(&[0xff]).await.unwrap();
        assert!(read_frame::<_, Value>(&mut reader).await.is_err());
    }

    #[tokio::test]
    async fn end_of_stream_is_none() {
        let (writer, mut reader) = duplex(16);
        drop(writer);
        assert!(read_frame::<_, Value>(&mut reader).await.unwrap().is_none());
    }
}
