import React from "react";

export function View({ name }: { name: string }) {
  if (!name) {
    return null;
  }
  return <p>{name}</p>;
}
