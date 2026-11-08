// §166: Docker page types. Mirrors crates/ols-core/src/docker/types.rs.

/** Engine check result; the UI adds a local "starting" state on top. */
export type DockerState = 'not_installed' | 'stopped' | 'error' | 'running'

export type DockerStatus = {
  state: DockerState
  installed: boolean
  running: boolean
  version: string | null
  api_version: string | null
  os_type: string | null
  endpoint: string
  desktop_path: string | null
  message: string | null
}

export type DockerPortBinding = {
  container_port: number
  protocol: string
  host_ip: string | null
  host_port: number | null
}

export type DockerContainer = {
  id: string
  name: string
  image: string
  state: string
  status: string
  created: number
  ports: DockerPortBinding[]
  compose_project: string | null
  compose_service: string | null
  compose_file?: string | null
  ols_site: string | null
  managed: boolean
}

export type DockerContainerAction = 'start' | 'stop' | 'restart' | 'pause' | 'unpause'

export type ComposePort = { host: number | null; container: number }

export type ComposeService = {
  name: string
  image: string | null
  build: boolean
  ports: ComposePort[]
  state: string | null
  container: string | null
  container_id?: string | null
}

export type ComposeStack = {
  name: string
  file: string | null
  dir: string | null
  project_id: string | null
  project_name: string | null
  services: ComposeService[]
  running: number
  file_error: string | null
  uses_secrets: boolean
  connected?: boolean
}

export type ComposeCheck = { ok: boolean; errors: string[]; warnings: string[]; docker_checked: boolean }

export type ComposeAction = 'up' | 'down' | 'stop' | 'start' | 'restart' | 'pull' | 'build' | 'watch'

export type PresetEnvValue = { kind: 'literal'; value: string } | { kind: 'secret' }

export type ServicePreset = {
  id: string
  name: string
  category: string
  description: string
  image: string
  tag: string
  tags: string[]
  default_service: string
  ports: { container: number; default_host: number; label: string; web: boolean }[]
  env: { key: string; value: PresetEnvValue }[]
  volumes: { source: string; target: string; read_only: boolean }[]
  command: string | null
  working_dir: string | null
  custom?: boolean
}

export type BuildService = {
  preset: string
  name: string
  tag?: string | null
  host_ports?: (number | null)[]
  image?: string | null
  container_port?: number | null
  env?: Record<string, string>
}

export type BuildSpec = { dir: string; name?: string | null; services: BuildService[] }

export type BuildPlan = {
  file: string
  exists: boolean
  yaml: string
  ports: { service: string; label: string; host: number; container: number; web: boolean }[]
  secrets: string[]
  warnings: string[]
}

export type DockerizeMode = 'dev' | 'prod'

export type DockerizeOptions = { mode?: DockerizeMode; services?: string[] | null }

export type DockerizeService = {
  id: string
  name: string
  service: string
  image: string
  reason: string
  detected: boolean
  selected: boolean
  optional: boolean
}

export type DockerizePlan = {
  kind: 'node' | 'php' | 'python' | 'static' | 'go' | 'rust' | 'ruby' | 'java' | 'dotnet'
  mode: DockerizeMode
  dockerfile: string | null
  compose: string
  dockerignore: string | null
  services: DockerizeService[]
  workers: string[]
  secrets: string[]
  watch: boolean
  dockerfile_path: string
  compose_path: string
  existing: string[]
  port: number
  notes: string[]
  php_extensions: string[]
  unknown_extensions: string[]
  detected: string[]
  disclaimer: string
  checklist: string[]
}

export type DockerSiteRequest = { container: string; container_port: number; hostname: string; https: boolean }

export type OlsEndpoint = {
  id: string
  name: string
  running: boolean
  port: number | null
  address: string | null
  connection: string | null
}

export type ConnectInfo = {
  enabled: boolean
  hostnames: string[]
  ca: boolean
  ca_path_in_container: string
  services: OlsEndpoint[]
  override_yaml: string
}

export type ContainerDetails = {
  id: string
  name: string
  image: string
  created: string
  state: string
  started_at: string | null
  health: string | null
  restart_count: number
  command: string[]
  entrypoint: string[]
  working_dir: string | null
  env: { key: string; value: string; masked: boolean }[]
  ports: DockerPortBinding[]
  mounts: { kind: string; source: string; target: string; read_only: boolean }[]
  networks: { name: string; ip: string | null; aliases: string[] }[]
  labels: Record<string, string>
  restart_policy: string
  max_retries: number
  memory_mb: number
  cpus: number
  compose_project: string | null
}

export type ContainerUpdate = {
  restart_policy?: string | null
  max_retries?: number | null
  memory_mb?: number | null
  cpus?: number | null
}

/** `value: null` keeps the container's current value (hidden passwords); a key left out is removed. */
export type EnvEdit = { key: string; value: string | null }

export type DockerImage = {
  id: string
  tags: string[]
  digest: string | null
  size: number
  created: number
  used_by: string[]
  dangling: boolean
}

export type DockerVolume = {
  name: string
  driver: string
  created: string | null
  anonymous: boolean
  compose_project: string | null
  used_by: string[]
  size: number | null
}

export type DockerDiskUsage = {
  images: number
  images_reclaimable: number
  containers: number
  containers_reclaimable: number
  volumes: number
  volumes_reclaimable: number
  build_cache: number
  build_cache_reclaimable: number
}

export type PruneTargets = {
  stopped_containers?: boolean
  dangling_images?: boolean
  unused_images?: boolean
  unused_volumes?: boolean
  build_cache?: boolean
}

export type PrunePlan = { items: { kind: string; name: string; size: number }[]; total: number }

export type RunSpec = {
  image: string
  name?: string | null
  ports?: [number, number][]
  env?: [string, string][]
  restart_policy?: string | null
}
