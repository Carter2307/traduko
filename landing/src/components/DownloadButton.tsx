import { Download } from 'lucide-react'

export default function DownloadButton({ compact = false }: { compact?: boolean }) {
  return (
    <button type="button" className={`download-button ${compact ? 'download-button-small' : ''}`} aria-label="Download for Mac, coming soon">
      {!compact && <Download size={18} strokeWidth={2} aria-hidden="true" />}
      <span>Download for Mac</span>
    </button>
  )
}
