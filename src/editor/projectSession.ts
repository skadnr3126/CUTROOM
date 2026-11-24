import { invoke as nativeInvoke, type InvokeArgs, type InvokeOptions } from "@tauri-apps/api/core";

export type EditorSeek = { session: string; sceneId: string; atMs: number };

export type ProjectSession = {
  id: string; session: string; path: string; legacy: boolean; draft: unknown; notice: string | null;
};

let binding: Promise<ProjectSession> | undefined;
export function openSession() {
  return binding ??= nativeInvoke<ProjectSession>("project_session").catch(error => {
    binding = undefined;
    throw error;
  });
}

export async function invoke<T>(command: string, args?: InvokeArgs, options?: InvokeOptions): Promise<T> {
  const project = await openSession();
  const headers = new Headers(options?.headers);
  headers.set("x-cutroom-session", project.session);
  const values: Record<string,string> = {};
  headers.forEach((value, key) => { values[key] = value; });
  return nativeInvoke<T>(command, args, { ...options, headers: values });
}

