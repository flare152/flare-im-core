use tonic::{Streaming, IntoStreamingRequest};
use flare_proto::media::UploadFileRequest;

fn test_streaming_trait() {
    // This is just to verify if Streaming implements IntoStreamingRequest
    // It won't compile if it doesn't
    let _stream: Streaming<UploadFileRequest> = unimplemented!();
    let _request = _stream.into_streaming_request();
}