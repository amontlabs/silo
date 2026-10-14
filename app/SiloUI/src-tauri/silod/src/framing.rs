use std::io;

use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

pub async fn read_json_frame<R, T>(reader: &mut R) -> io::Result<Option<T>>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut header = [0_u8; 4];
    let read = reader.read(&mut header[..1]).await?;
    if read == 0 {
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

pub async fn write_json_frame<W, T>(writer: &mut W, value: &T) -> io::Result<()>
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
    use serde_json::Value;
    use tokio::io::{duplex, AsyncWriteExt};

    #[tokio::test]
    async fn reads_a_header_and_json_split_across_writes() {
        let (mut client, mut server) = duplex(64);
        let writer = tokio::spawn(async move {
            client.write_all(&[0]).await.unwrap();
            client.write_all(&[0, 0]).await.unwrap();
            client.write_all(&[10]).await.unwrap();
            client.write_all(b"{\"id\":\"x\"}").await.unwrap();
        });
        let value: Value = read_json_frame(&mut server).await.unwrap().unwrap();
        writer.await.unwrap();
        assert_eq!(value["id"], "x");
    }

    #[tokio::test]
    async fn rejects_oversized_frames_before_reading_the_payload() {
        let (mut writer, mut reader) = duplex(16);
        writer
            .write_all(&((MAX_FRAME_BYTES as u32 + 1).to_be_bytes()))
            .await
            .unwrap();
        let error = read_json_frame::<_, Value>(&mut reader).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn rejects_invalid_utf8_json_and_zero_length_frames() {
        let (mut writer, mut reader) = duplex(16);
        writer.write_all(&1_u32.to_be_bytes()).await.unwrap();
        writer.write_all(&[0xff]).await.unwrap();
        assert_eq!(
            read_json_frame::<_, Value>(&mut reader)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );

        let (mut writer, mut reader) = duplex(16);
        writer.write_all(&0_u32.to_be_bytes()).await.unwrap();
        assert_eq!(
            read_json_frame::<_, Value>(&mut reader)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[tokio::test]
    async fn writes_the_v5_derived_four_byte_big_endian_length_prefix() {
        let (mut writer, mut reader) = duplex(64);
        let value = serde_json::json!({ "id": "x" });
        write_json_frame(&mut writer, &value).await.unwrap();
        let mut header = [0_u8; 4];
        reader.read_exact(&mut header).await.unwrap();
        let length = u32::from_be_bytes(header) as usize;
        assert_eq!(length, b"{\"id\":\"x\"}".len());
        let mut payload = vec![0; length];
        reader.read_exact(&mut payload).await.unwrap();
        assert_eq!(payload, b"{\"id\":\"x\"}");
    }
}
