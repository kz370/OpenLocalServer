import type { ServicePreset } from '@/core'

/** A blank preset, or one started from an image someone typed in the builder. */
export function blankPreset(image = '', containerPort: number | null = null): ServicePreset {
  // `nginx:1.28` -> image `nginx`, tag `1.28`. A registry port (`host:5000/app`) stays in the name.
  const lastSlash = image.lastIndexOf('/')
  const colon = image.lastIndexOf(':')
  const hasTag = colon > lastSlash
  return {
    id: '',
    name: '',
    category: 'My presets',
    description: '',
    image: hasTag ? image.slice(0, colon) : image,
    tag: '',
    tags: hasTag ? [image.slice(colon + 1)] : [],
    default_service: 'app',
    ports: containerPort ? [{ container: containerPort, default_host: Math.max(containerPort, 1024), label: 'Web', web: true }] : [],
    env: [],
    volumes: [],
    command: null,
    working_dir: null,
    custom: true,
  }
}
