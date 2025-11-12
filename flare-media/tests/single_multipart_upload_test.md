# 单独分片上传测试说明

## 概述

这个测试 (`test_single_multipart_upload_flow`) 演示了Flare Media Service中完整的分片上传流程。它涵盖了从初始化分片上传到上传多个分片再到完成整个上传过程的所有步骤。

## 测试流程

测试按以下步骤执行：

1. **初始化分片上传**
   - 创建一个512KB大小的文件元数据
   - 设置期望的分片大小为256KB
   - 调用 `InitiateMultipartUpload` 接口
   - 验证返回的upload_id和chunk_size

2. **上传第一个分片**
   - 创建256KB的数据块（填充为1）
   - 设置分片索引为0
   - 调用 `UploadMultipartChunk` 接口
   - 验证上传结果

3. **上传第二个分片**
   - 创建256KB的数据块（填充为2）
   - 设置分片索引为1
   - 调用 `UploadMultipartChunk` 接口
   - 验证上传结果

4. **完成分片上传**
   - 使用相同的upload_id调用 `CompleteMultipartUpload` 接口
   - 验证返回的file_id和url不为空
   - 确认文件信息存在

## 运行测试

```bash
cd /Users/hg/workspace/flare/flare-im/flare-im-core/flare-media
cargo test test_single_multipart_upload_flow -- --nocapture
```

## 预期结果

测试应该成功完成，显示每个步骤的日志信息，并最终输出"完整分片上传流程测试成功完成！"。