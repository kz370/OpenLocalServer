// §166: Docker page types. Mirrors crates/ols-core/src/docker/types.rs.

export type DockerStatus = {
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

export type ComposeAction = 'up' | 'down' | 'stop' | 'start' | 'restart' | 'pull' | 'build'

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
