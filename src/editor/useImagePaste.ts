import { useEffect } from "react";

const extensions: Record<string, string> = { "image/png": "png", "image/jpeg": "jpg", "image/webp": "webp" };

function pastedImages(data: DataTransfer): File[] {
  const files = data.files.length ? Array.from(data.files) :
    Array.from(data.items).flatMap(item => {
      const file = item.kind === "file" ? item.getAsFile() : null;
      return file ? [file] : [];
    });
  const images = files.filter(file => file.type.startsWith("image/") ||
    (!file.type && /\.(png|jpe?g|webp)$/i.test(file.name)));
  return images.map((file, index) => {
    // Chromium assigns a generic filename to image pixels copied by a capture tool.
    if (file.name && !/^image(\.[a-z0-9]+)?$/i.test(file.name) && /\.[^./]+$/.test(file.name)) return file;
    const extension = extensions[file.type];
    if (!extension) return file;
    const stamp = new Date().toLocaleString("sv-SE").replace(/:/g, "-");
    const number = images.length > 1 ? ` (${index + 1})` : "";
    return new File([file], `붙여넣은 이미지 ${stamp}${number}.${extension}`, { type: file.type });
  });
}

export function useImagePaste(onImages: (images: File[]) => void) {
  useEffect(() => {
    function handlePaste(event: ClipboardEvent) {
      if (event.defaultPrevented || !event.clipboardData) return;
      const images = pastedImages(event.clipboardData);
      if (!images.length) return;
      event.preventDefault();
      onImages(images);
    }
    window.addEventListener("paste", handlePaste);
    return () => window.removeEventListener("paste", handlePaste);
  }, [onImages]);
}
