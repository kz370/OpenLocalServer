import { createContext } from 'react'

/** Tracks modal nesting depth so inner dialogs are automatically layered above outer dialogs. */
export const DialogDepthContext = createContext<number>(0)
