const types: Record<string, string> = {
  pdf: 'application/pdf',
  docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  xlsx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
  md: 'text/markdown',
  txt: 'text/plain',
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  webp: 'image/webp',
};

export function attachmentContentType(file: File): string {
  const extension = file.name.split('.').pop()?.toLowerCase() ?? '';
  const contentType = types[extension];
  if (!contentType) throw new Error('请选择 PDF、DOCX、XLSX、MD、TXT 或图片文件。');
  return contentType;
}
