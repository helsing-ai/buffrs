import Link from 'next/link';
import { Heading } from '@/components/Heading'

const fresh = [
  { name: 'units', version: 'v0.2.2' },
  { name: 'chrono', version: 'v0.0.1' },
  { name: 'codecs', version: 'v0.2.0' },
  { name: 'buffrs', version: 'v0.6.3' },
]

const popular = [
  { name: 'github' },
  { name: 'google' },
  { name: 'reflection' },
  { name: 'buffrs-registry' },
]

const released = [
  { name: 'gitlab', version: 'v2.2.2' },
  { name: 'some-api', version: 'v2.0.1' },
  { name: 'radio', version: 'v0.1.0' },
  { name: 'waveform', version: 'v4.2.1' },
]

export const Packages = () => {
  return (
    <div className="my-16 xl:max-w-none">
      <Heading level={2} id="categories" anchor={false}>
        <Link href="/packages">
          Packages
        </Link>
      </Heading>
      <div className="not-prose mt-4 grid grid-cols-1 md:grid-cols-3 gap-5 border-t border-zinc-900/5 pt-6 dark:border-white/5">
        <div>
          <h4 className="text-xs font-bold">New Packages</h4>
          <div className="not-prose mt-4 grid grid-cols-1 gap-5">
            {fresh.map((item) => (
              <Link href={`/packages/${item.name}`}>
                <div key={item.name} className="overflow-hidden rounded-lg bg-white dark:bg-white/2.5 px-4 py-5 border border-zinc-900/5 dark:border-white/5 sm:p-6">
                  <dt className="truncate text-sm font-medium text-zinc-600 dark:text-zinc-400">
                    {item.name}
                    <span className="ml-2 text-xs font-light">@{' '}{item.version}</span>
                  </dt>
                </div>
              </Link>
            ))}
          </div>
        </div>
        <div>
          <h4 className="text-xs font-bold">Just Released</h4>
          <div className="not-prose mt-4 grid grid-cols-1 gap-5">
            {released.map((item) => (
              <Link href={`/packages/${item.name}`}>
                <div key={item.name} className="overflow-hidden rounded-lg bg-white dark:bg-white/2.5 px-4 py-5 border border-zinc-900/5 dark:border-white/5 sm:p-6">
                  <dt className="truncate text-sm font-medium text-zinc-600 dark:text-zinc-400">
                    {item.name}
                    <span className="ml-2 text-xs font-light">@{' '}{item.version}</span>
                  </dt>
                </div>
              </Link>
            ))}
          </div>
        </div>
        <div>
          <h4 className="text-xs font-bold">Popular</h4>
          <div className="not-prose mt-4 grid grid-cols-1 gap-5">
            {popular.map((item) => (
              <Link href={`/packages/${item.name}`}>
                <div key={item.name} className="overflow-hidden rounded-lg bg-white dark:bg-white/2.5 px-4 py-5 border border-zinc-900/5 dark:border-white/5 sm:p-6">
                  <dt className="truncate text-sm font-medium text-zinc-600 dark:text-zinc-400">
                    {item.name}
                  </dt>
                </div>
              </Link>
            ))}
          </div>
        </div>
      </div>
    </div>
  )
}
