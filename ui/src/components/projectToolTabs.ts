export type ToolTab = 'environment' | 'terminal' | 'dumps' | 'env' | 'git' | 'workers' | 'backups' | 'repair' | 'mail' | 'composer' | 'node' | 'python' | 'xdebug' | 'profiler' | 'load'

/** The project tools, in the order the site dialog lists them. */
export const TOOL_TABS: { id: ToolTab; label: string }[] = [
  { id: 'environment', label: 'Environment' },
  { id: 'terminal', label: 'Terminal' },
  { id: 'dumps', label: 'Dumps' },
  { id: 'env', label: '.env' },
  { id: 'git', label: 'Git' },
  { id: 'workers', label: 'Workers' },
  { id: 'backups', label: 'Backups' },
  { id: 'repair', label: 'Repair' },
  { id: 'mail', label: 'Mail' },
  { id: 'composer', label: 'Composer' },
  { id: 'node', label: 'Node' },
  { id: 'python', label: 'Python' },
  { id: 'xdebug', label: 'Xdebug' },
  { id: 'profiler', label: 'Profiler' },
  { id: 'load', label: 'Load testing' },
]
