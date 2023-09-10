import Link from 'next/link'

import { Tag } from '@/components/Tag'
import { Button } from '@/components/Button'
import { Hero } from '@/components/hero/Hero'

import { Statistics } from '@/components/Statistics'
import { Categories } from '@/components/Categories'

type Package = {
  name: string;
  description: string;
  version: string;

  downloads: number;
  versions: number;
  updatedAt: string;
};

const packages: [Package] = [
  { name: "buffrs-registry", description: "A protobuf package registry", version: "v1.0.0", downloads: 283, versions: 4, updatedAt: "4 days ago" },
  { name: "buffrs-registry", description: "A protobuf package registry", version: "v1.0.0", downloads: 283, versions: 4, updatedAt: "4 days ago" },
  { name: "buffrs-registry", description: "A protobuf package registry", version: "v1.0.0", downloads: 283, versions: 4, updatedAt: "4 days ago" },
  { name: "buffrs-registry", description: "A protobuf package registry", version: "v1.0.0", downloads: 283, versions: 4, updatedAt: "4 days ago" },
  { name: "buffrs-registry", description: "A protobuf package registry", version: "v1.0.0", downloads: 283, versions: 4, updatedAt: "4 days ago" },
  { name: "buffrs-registry", description: "A protobuf package registry", version: "v1.0.0", downloads: 283, versions: 4, updatedAt: "4 days ago" },
  { name: "buffrs-registry", description: "A protobuf package registry", version: "v1.0.0", downloads: 283, versions: 4, updatedAt: "4 days ago" },
  { name: "buffrs-registry", description: "A protobuf package registry", version: "v1.0.0", downloads: 283, versions: 4, updatedAt: "4 days ago" },
];

const Item = ({ pkg }: { pkg: Package }): React.Element => {
  return (
    <Link href={`/packages/${pkg.name}`}>
      <li key={pkg.name} className="overflow-hidden flex rounded-lg bg-white dark:bg-white/2.5 px-4 py-5 border border-zinc-900/5 dark:border-white/5 sm:p-6">
        <div>
          <dt className="truncate text-sm font-medium text-zinc-600 dark:text-zinc-400">
            {pkg.name}
            <span className="ml-2 text-xs font-light">@{' '}{pkg.version}</span>
          </dt>

          <dt className="truncate text-sm font-light text-zinc-400 dark:text-zinc-400">
            {pkg.description}
          </dt>

          <div className="flex space-x-4">
            <Tag variant="small" color="rose" className="">
              API
            </Tag>

            <Tag variant="small" color="zinc" className="">
              #registry
            </Tag>

            <Tag variant="small" color="zinc" className="">
              #protobuf
            </Tag>
          </div>
        </div>

        <div className="ml-auto">
          <dt className="truncate text-sm font-light text-zinc-400 dark:text-zinc-400">
            Downloads: {pkg.downloads}
          </dt>
          <dt className="truncate text-sm font-light text-zinc-400 dark:text-zinc-400">
            Versions: {pkg.versions}
          </dt>

          <dt className="truncate text-sm font-light text-zinc-400 dark:text-zinc-400">
            Updated: {pkg.updatedAt}
          </dt>
        </div>
      </li>
    </Link>
  )
}

const List = (): React.Element => {
  return (
    <ul className="mt-4 flex flex-col space-y-4">
      {(packages.map(pkg => (
        <Item pkg={pkg} />
      )))}
    </ul>
  )
}

const Packages = (): React.Element => {
  return (
    <>
      <Hero title="Packages">

      </Hero>
      <List />
    </>
  )
}

export default Packages;