# Flare Media Service 集成测试指南

## 概述

这个集成测试套件验证了MediaService的所有gRPC接口功能，包括：

1. UploadFile：测试流式文件上传功能
2. 分片上传流程：InitiateMultipartUpload/UploadMultipartChunk/CompleteMultipartUpload
3. 引用管理功能：CreateReference/DeleteReference/ListReferences
4. 文件信息获取功能：GetFileUrl/GetFileInfo
5. 文件删除功能：DeleteFile
6. 媒体处理功能：ProcessImage/ProcessVideo

## 测试环境设置

要运行这些测试，您需要：

1. 确保已安装所有依赖项
2. 对于完整的功能测试，需要配置以下服务：
   - Redis（用于元数据缓存和上传会话存储）
   - PostgreSQL（用于元数据存储）
   - S3兼容对象存储（如MinIO）

## 运行测试

```bash
# 运行所有测试
cargo test -p flare-media --tests

# 运行特定测试
cargo test -p flare-media test_upload_file -- --nocapture
```

## 测试配置

测试使用模拟服务来验证接口的正确性。在实际部署中，您需要配置真实的服务后端。

## 当前测试状态

- [x] 基本框架和编译通过
- [ ] 完整功能验证（需要真实后端服务）
- [ ] 异常情况处理测试
- [ ] 性能测试

## 扩展测试

要添加更多测试用例，请参考现有的测试模式并添加新的测试函数。