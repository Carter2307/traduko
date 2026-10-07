import { useEffect, useState } from 'react'
import type { MouseEvent } from 'react'
import { Download } from 'lucide-react'
import { MAC_DOWNLOADS, resolveMacDownloadUrl } from '../lib/mac-download'
import type { MacClientHints } from '../lib/mac-download'

// Resolve once for all three download links. MacIntel and "Intel Mac OS X"
// appear on Apple silicon too, so only explicit architecture hints are used.
const downloadUrl = resolveMacDownloadUrl(
  typeof navigator === 'undefined'
    ? undefined
    : (navigator as Navigator & { userAgentData?: MacClientHints }).userAgentData,
)

export default function DownloadButton({ compact = false }: { compact?: boolean }) {
  const [href, setHref] = useState<string>(MAC_DOWNLOADS.appleSilicon)

  useEffect(() => {
    let active = true
    void downloadUrl.then((url) => { if (active) setHref(url) })
    return () => { active = false }
  }, [])

  const handleDownload = async (event: MouseEvent<HTMLAnchorElement>) => {
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return
    event.preventDefault()
    // A click during detection waits for the same bounded result, preventing
    // the initial Apple silicon href from winning a race on an Intel Mac.
    window.location.assign(await downloadUrl)
  }

  const architecture = href === MAC_DOWNLOADS.intel ? 'Intel' : 'Apple silicon'

  return (
    <a
      href={href}
      onClick={handleDownload}
      className={`download-button ${compact ? 'download-button-small' : ''}`}
      aria-label={`Download Traduko 0.2.0 for Mac (${architecture})`}
    >
      {!compact && <Download size={18} strokeWidth={2} aria-hidden="true" />}
      <span>Download for Mac</span>
    </a>
  )
}
