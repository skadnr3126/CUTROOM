import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

export function useCutroomEvent<T>(name: string, receive: (payload: T) => void) {
  const callback = useRef(receive);
  callback.current = receive;
  useEffect(() => {
    let active = true;
    const registration = listen<T>(name, event => {
      if (active) callback.current(event.payload);
    });
    return () => {
      active = false;
      void registration.then(unlisten => unlisten());
    };
  }, [name]);
}
