import { useEffect, useState } from 'react'

import { type NetworkStatus, runCommand } from '@/core'

/** §128: a small notice in the sidebar while the internet can't be reached. */
export function useOnline(): NetworkStatus | null {
  const [status, setStatus] = useState<NetworkStatus | null>(null)
  useEffect(() => {
    let alive = true
    const check = () => runCommand({ type: 'check_network', force: false }).then((r) => alive && r.type === 'network' && setStatus(r.status)).catch(() => { })
    check()
    const t = setInterval(check, 30000)
    return () => {
      alive = false
      clearInterval(t)
    }
  }, [])
  return status
}
