# Flare Media Service 完整集成测试指南

## 目录
1. [概述](#概述)
2. [测试环境配置](#测试环境配置)
3. [运行测试](#运行测试)
4. [测试用例详解](#测试用例详解)
5. [扩展测试](#扩展测试)

## 概述

Flare Media Service 提供了完整的媒体处理功能，包括文件上传、分片上传、引用管理、文件信息获取、文件删除和媒体处理等。本测试套件验证了所有gRPC接口的正确性。

## 测试环境配置

### 依赖服务

要运行完整的集成测试，您需要配置以下服务：

1. **Redis**: 用于元数据缓存和上传会话存储
2. **PostgreSQL**: 用于持久化元数据存储
3. **S3兼容对象存储**: 如MinIO、AWS S3等，用于文件存储

### 配置文件

创建测试配置文件 `config/test.toml`:

```toml
[server]
address = "0.0.0.0"
port = 60081

[service]
name = "flare-media-test"

[media-service]
redis_ttl_seconds = 3600
orphan_grace_seconds = 86400
chunk_ttl_seconds = 172800
max_chunk_size_bytes = 52428800

[media-service.metadata-cache]
profile = "redis-test"

[media-service.metadata-store]
profile = "postgres-test"

[media-service.object-store]
profile = "s3-test"

[media-service.upload-session-store]
profile = "redis-test"

[redis.redis-test]
url = "redis://localhost:6379"
namespace = "flare:media:test"

[postgres.postgres-test]
url = "postgresql://user:password@localhost:5432/flaredb_test"

[object-store.s3-test]
profile_type = "minio"
endpoint = "http://localhost:9000"
access_key = "minioadmin"
secret_key = "minioadmin"
bucket = "flare-media-test"
region = "us-east-1"
```

## 运行测试

### 基本测试运行

```bash
# 运行所有测试
cargo test -p flare-media --tests

# 运行特定测试
cargo test -p flare-media test_upload_file -- --nocapture

# 运行时显示详细日志
RUST_LOG=debug cargo test -p flare-media --tests
```

### 使用Docker环境

为了简化测试环境设置，您可以使用docker-compose启动依赖服务：

```bash
# 启动测试环境
docker-compose -f deploy/docker-compose.test.yml up -d

# 运行测试
cargo test -p flare-media --tests

# 停止测试环境
docker-compose -f deploy/docker-compose.test.yml down
```

## 测试用例详解

### 1. 文件上传测试 (UploadFile)

验证流式文件上传功能的正确性：

```rust
#[tokio::test]
async fn test_upload_file() -> Result<()> {
    // 设置测试环境
    let addr = start_test_server().await?;
    let mut client = create_test_client(addr).await?;
    
    // 创建测试数据
    let metadata = UploadFileMetadata {
        file_name: "test.txt".to_string(),
        mime_type: "text/plain".to_string(),
        file_size: 1024,
        file_type: FileType::Other as i32,
        // ... 其他字段
    };
    
    // 执行上传操作
    let requests = vec![
        UploadFileRequest {
            request: Some(upload_file_request::Request::Metadata(metadata)),
        },
        UploadFileRequest {
            request: Some(upload_file_request::Request::ChunkData(vec![1, 2, 3, 4, 5])),
        },
    ];
    
    let request = tonic::Request::new(tokio_stream::iter(requests));
    let response = client.upload_file(request).await?;
    
    // 验证结果
    let response = response.into_inner();
    assert!(response.success);
    assert!(!response.file_id.is_empty());
    
    Ok(())
}
```

### 2. 分片上传测试

验证分片上传流程的正确性：

```rust
#[tokio::test]
async fn test_multipart_upload() -> Result<()> {
    // 1. 初始化分片上传
    let init_request = InitiateMultipartUploadRequest {
        metadata: Some(UploadFileMetadata {
            file_name: "multipart-test.txt".to_string(),
            // ... 其他字段
        }),
        desired_chunk_size: 1024,
        // ... 其他字段
    };
    
    let init_response = client.initiate_multipart_upload(init_request).await?;
    let init_response = init_response.into_inner();
    let upload_id = init_response.upload_id;
    
    // 2. 上传分片
    let chunk_request = UploadMultipartChunkRequest {
        upload_id: upload_id.clone(),
        chunk_index: 0,
        payload: vec![1; 1024],
        // ... 其他字段
    };
    
    let chunk_response = client.upload_multipart_chunk(chunk_request).await?;
    let chunk_response = chunk_response.into_inner();
    
    // 3. 完成分片上传
    let complete_request = CompleteMultipartUploadRequest {
        upload_id: upload_id.clone(),
        // ... 其他字段
    };
    
    let complete_response = client.complete_multipart_upload(complete_request).await?;
    let complete_response = complete_response.into_inner();
    
    Ok(())
}
```

### 3. 引用管理测试

验证文件引用管理功能：

```rust
#[tokio::test]
async fn test_reference_management() -> Result<()> {
    // 1. 创建引用
    let create_request = CreateReferenceRequest {
        file_id: "test-file-id".to_string(),
        namespace: "test-namespace".to_string(),
        owner_id: "test-owner".to_string(),
        // ... 其他字段
    };
    
    let create_response = client.create_reference(create_request).await?;
    let create_response = create_response.into_inner();
    
    // 2. 列出引用
    let list_request = ListReferencesRequest {
        file_id: "test-file-id".to_string(),
        // ... 其他字段
    };
    
    let list_response = client.list_references(list_request).await?;
    let list_response = list_response.into_inner();
    
    // 3. 删除引用
    let delete_request = DeleteReferenceRequest {
        file_id: "test-file-id".to_string(),
        reference_id: "test-reference-id".to_string(),
        // ... 其他字段
    };
    
    let delete_response = client.delete_reference(delete_request).await?;
    let delete_response = delete_response.into_inner();
    
    Ok(())
}
```

### 4. 文件操作测试

验证文件信息获取和删除功能：

```rust
#[tokio::test]
async fn test_file_operations() -> Result<()> {
    // 1. 获取文件信息
    let info_request = GetFileInfoRequest {
        file_id: "test-file-id".to_string(),
        // ... 其他字段
    };
    
    let info_response = client.get_file_info(info_request).await?;
    let info_response = info_response.into_inner();
    
    // 2. 获取文件URL
    let url_request = GetFileUrlRequest {
        file_id: "test-file-id".to_string(),
        expires_in: 3600,
        // ... 其他字段
    };
    
    let url_response = client.get_file_url(url_request).await?;
    let url_response = url_response.into_inner();
    
    // 3. 删除文件
    let delete_request = DeleteFileRequest {
        file_id: "test-file-id".to_string(),
        // ... 其他字段
    };
    
    let delete_response = client.delete_file(delete_request).await?;
    let delete_response = delete_response.into_inner();
    
    Ok(())
}
```

### 5. 媒体处理测试

验证图片和视频处理功能：

```rust
#[tokio::test]
async fn test_media_processing() -> Result<()> {
    // 1. 处理图片
    let resize_op = ResizeOperation {
        width: 800,
        height: 600,
        keep_aspect_ratio: true,
    };
    
    let compress_op = CompressOperation {
        quality: 80,
    };
    
    let image_request = ProcessImageRequest {
        file_id: "test-image-id".to_string(),
        operations: vec![
            ImageOperation {
                operation: Some(image_operation::Operation::Resize(resize_op)),
            },
            ImageOperation {
                operation: Some(image_operation::Operation::Compress(compress_op)),
            },
        ],
        // ... 其他字段
    };
    
    let image_response = client.process_image(image_request).await?;
    let image_response = image_response.into_inner();
    
    // 2. 处理视频
    let transcode_op = TranscodeOperation {
        format: "mp4".to_string(),
        quality: "medium".to_string(),
    };
    
    let video_request = ProcessVideoRequest {
        file_id: "test-video-id".to_string(),
        operations: vec![
            VideoOperation {
                operation: Some(video_operation::Operation::Transcode(transcode_op)),
            },
        ],
        // ... 其他字段
    };
    
    let video_response = client.process_video(video_request).await?;
    let video_response = video_response.into_inner();
    
    Ok(())
}
```

## 扩展测试

### 添加新的测试用例

要添加新的测试用例，请按照以下步骤操作：

1. 在 `tests/integration_test.rs` 文件中添加新的测试函数
2. 遵循现有的测试模式
3. 确保测试覆盖正常流程和异常情况

```rust
#[tokio::test]
async fn test_new_feature() -> Result<()> {
    // 设置测试环境
    let addr = start_test_server().await?;
    let mut client = create_test_client(addr).await?;
    
    // 执行测试操作
    // ...
    
    // 验证结果
    // ...
    
    Ok(())
}
```

### 测试异常情况

添加异常情况测试以确保服务的健壮性：

```rust
#[tokio::test]
async fn test_upload_file_invalid_metadata() -> Result<()> {
    let addr = start_test_server().await?;
    let mut client = create_test_client(addr).await?;
    
    // 创建无效的元数据
    let metadata = UploadFileMetadata {
        file_name: "".to_string(), // 无效文件名
        // ... 其他字段
    };
    
    let requests = vec![
        UploadFileRequest {
            request: Some(upload_file_request::Request::Metadata(metadata)),
        },
    ];
    
    let request = tonic::Request::new(tokio_stream::iter(requests));
    let response = client.upload_file(request).await;
    
    // 验证错误处理
    assert!(response.is_err());
    
    Ok(())
}
```

### 性能测试

添加性能测试以验证服务在高负载下的表现：

```rust
#[tokio::test]
async fn test_concurrent_uploads() -> Result<()> {
    let addr = start_test_server().await?;
    
    // 并发执行多个上传操作
    let mut handles = vec![];
    for i in 0..10 {
        let addr = addr.clone();
        let handle = tokio::spawn(async move {
            let mut client = create_test_client(addr).await?;
            // 执行上传操作
            // ...
            Ok::<_, anyhow::Error>(())
        });
        handles.push(handle);
    }
    
    // 等待所有操作完成
    for handle in handles {
        handle.await??;
    }
    
    Ok(())
}
```

## 常见问题

### 测试失败

如果测试失败，请检查：

1. 所有依赖服务是否正常运行
2. 配置文件是否正确
3. 网络连接是否正常
4. 权限设置是否正确

### 日志调试

启用详细日志以帮助调试：

```bash
RUST_LOG=debug cargo test -p flare-media --tests -- --nocapture
```

### 测试环境清理

确保在每次测试运行后清理测试数据：

```rust
#[tokio::test]
async fn test_with_cleanup() -> Result<()> {
    // 设置测试环境
    // ...
    
    // 执行测试
    // ...
    
    // 清理测试数据
    cleanup_test_data().await?;
    
    Ok(())
}
```