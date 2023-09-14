import { Heading } from '@/components/Heading'

const stats = [
  { name: 'Packages Served', stat: '71897' },
  { name: 'Total Downloads', stat: '5863287576' },
  { name: 'Teams', stat: '24' },
  { name: 'Users', stat: '2234' },
]

export const Statistics = () => {
  return (
    <div className="my-16 xl:max-w-none">
      <Heading level={2} id="categories">
        Statistics
      </Heading>
      <div className="not-prose mt-4 grid grid-cols-1 gap-5 sm:grid-cols-2 xl:grid-cols-4 border-t border-zinc-900/5 pt-10 dark:border-white/5">
            {stats.map((item) => (

              <div key={item.name} className="overflow-hidden rounded-lg bg-white dark:bg-white/2.5 px-4 py-5 border border-zinc-900/5 dark:border-white/5 sm:p-6">
                <dt className="truncate text-sm font-medium text-zinc-600 dark:text-zinc-400">{item.name}</dt>
                <dd className="mt-1 text-xl font-semibold tracking-tight text-zinc-900 dark:text-white">{item.stat}</dd>
              </div>
            ))}
      </div>
    </div>
  )
}
