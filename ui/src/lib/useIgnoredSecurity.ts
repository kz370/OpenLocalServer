import { useCallback, useEffect, useState } from 'react'

import { runCommand } from '@/core'
import { SECURITY_IGNORED_SETTING, parseIgnored, withIgnored } from '@/lib/securityIgnore'

/** The hidden security items, saved in app settings so they stay hidden after a restart. */
export function useIgnoredSecurity() {
  const [ignored, setIgnored] = useState<string[]>([])

  useEffect(() => {
    void runCommand({ type: 'get_setting', key: SECURITY_IGNORED_SETTING })
      .then((r) => {
        if (r.type === 'setting') setIgnored(parseIgnored(r.value))
      })
      .catch(() => undefined)
  }, [])

  const save = useCallback((next: string[]) => {
    setIgnored(next)
    void runCommand({ type: 'set_setting', key: SECURITY_IGNORED_SETTING, value: next }).catch(() => undefined)
  }, [])

  const setHidden = useCallback((key: string, hidden: boolean) => save(withIgnored(ignored, key, hidden)), [ignored, save])
  const showAll = useCallback(() => save([]), [save])
  return { ignored, setHidden, showAll }
}
