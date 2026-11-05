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
  ols_site: string | null
  managed: boolean
}

export type DockerContainerAction = 'start' | 'stop' | 'restart' | 'pause' | 'unpause'
