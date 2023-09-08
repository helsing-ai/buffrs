'use client'

import Link from 'next/link'
import {
  type MotionValue,
  motion,
  useMotionTemplate,
  useMotionValue,
} from 'framer-motion'

import { GridPattern } from '@/components/GridPattern'
import { Heading } from '@/components/Heading'
import { ChatBubbleIcon } from '@/components/icons/ChatBubbleIcon'
import { EnvelopeIcon } from '@/components/icons/EnvelopeIcon'
import { UserIcon } from '@/components/icons/UserIcon'
import { UsersIcon } from '@/components/icons/UsersIcon'

interface Category {
  href: string
  name: string
  description: string
  packages: number
  icon: React.ComponentType<{ className?: string }>
  pattern: Omit<
    React.ComponentPropsWithoutRef<typeof GridPattern>,
    'width' | 'height' | 'x'
  >
}

const categories: Array<Category> = [
  {
    href: '/category/aerospace',
    name: 'Aerospace',
    description:
      'Packages for aeronautics (within the atmosphere) and astronautics (in outer space) implications.',
    packages: 281,
    icon: EnvelopeIcon,
    pattern: {
      y: 32,
      squares: [
        [0, 2],
        [1, 4],
      ],
    },
  },
  {
    href: '/category/computer-vision',
    name: 'Computer Vision',
    description:
      'Packages for describing the world of videos, codecs, pixels, images and so on.',
    packages: 23423,
    icon: UsersIcon,
    pattern: {
      y: 22,
      squares: [[0, 1]],
    },
  },
  {
    href: '/category/science',
    name: 'Science',
    description:
      'Packages related to solving problems involving physics, chemistry, biology, machine learning, geoscience, and other scientific fields.',
    packages: 93,
    icon: EnvelopeIcon,
    pattern: {
      y: 32,
      squares: [
        [0, 2],
        [1, 4],
      ],
    },
  },
  {
    href: '/category/finance',
    name: 'Finance',
    description:
      'Packages for dealing with money. Accounting, trading, investments, taxes, banking and payment processing using government-backed currencies.',
    packages: 7540,
    icon: UsersIcon,
    pattern: {
      y: 22,
      squares: [[0, 1]],
    },
  },
]

function CategoryIcon({ icon: Icon }: { icon: Category['icon'] }) {
  return (
    <div className="flex h-7 w-7 items-center justify-center rounded-full bg-zinc-900/5 ring-1 ring-zinc-900/25 backdrop-blur-[2px] transition duration-300 group-hover:bg-white/50 group-hover:ring-zinc-900/25 dark:bg-white/7.5 dark:ring-white/15 dark:group-hover:bg-purple-300/10 dark:group-hover:ring-purple-400">
      <Icon className="h-5 w-5 fill-zinc-700/10 stroke-zinc-700 transition-colors duration-300 group-hover:stroke-zinc-900 dark:fill-white/10 dark:stroke-zinc-400 dark:group-hover:fill-purple-300/10 dark:group-hover:stroke-purple-400" />
    </div>
  )
}

function CategoryPattern({
  mouseX,
  mouseY,
  ...gridProps
}: Category['pattern'] & {
  mouseX: MotionValue<number>
  mouseY: MotionValue<number>
}) {
  let maskImage = useMotionTemplate`radial-gradient(180px at ${mouseX}px ${mouseY}px, white, transparent)`
  let style = { maskImage, WebkitMaskImage: maskImage }

  return (
    <div className="pointer-events-none">
      <div className="absolute inset-0 rounded-2xl transition duration-300 [mask-image:linear-gradient(white,transparent)] group-hover:opacity-50">
        <GridPattern
          width={72}
          height={56}
          x="50%"
          className="absolute inset-x-0 inset-y-[-30%] h-[160%] w-full skew-y-[-18deg] fill-black/[0.02] stroke-black/5 dark:fill-white/1 dark:stroke-white/2.5"
          {...gridProps}
        />
      </div>
      <motion.div
        className="absolute inset-0 rounded-2xl bg-gradient-to-r from-blue-500/20 to-purple-500/20 opacity-0 transition duration-300 group-hover:opacity-100 dark:from-blue-500/30 dark:to-purple-500/30"
        style={style}
      />
      <motion.div
        className="absolute inset-0 rounded-2xl opacity-0 mix-blend-overlay transition duration-300 group-hover:opacity-100"
        style={style}
      >
        <GridPattern
          width={72}
          height={56}
          x="50%"
          className="absolute inset-x-0 inset-y-[-30%] h-[160%] w-full skew-y-[-18deg] fill-black/50 stroke-black/70 dark:fill-white/2.5 dark:stroke-white/10"
          {...gridProps}
        />
      </motion.div>
    </div>
  )
}

function Category({ category }: { category: Category }) {
  let mouseX = useMotionValue(0)
  let mouseY = useMotionValue(0)

  function onMouseMove({
    currentTarget,
    clientX,
    clientY,
  }: React.MouseEvent<HTMLDivElement>) {
    let { left, top } = currentTarget.getBoundingClientRect()
    mouseX.set(clientX - left)
    mouseY.set(clientY - top)
  }

  return (
    <div
      key={category.href}
      onMouseMove={onMouseMove}
      className="group relative flex rounded-2xl bg-zinc-50 transition-shadow hover:shadow-md hover:shadow-zinc-900/5 dark:bg-white/2.5 dark:hover:shadow-black/5"
    >
      <CategoryPattern {...category.pattern} mouseX={mouseX} mouseY={mouseY} />
      <div className="absolute inset-0 rounded-2xl ring-1 ring-inset ring-zinc-900/7.5 group-hover:ring-zinc-900/10 dark:ring-white/10 dark:group-hover:ring-white/20" />
      <div className="relative rounded-2xl px-4 pb-4 pt-16">
        <CategoryIcon icon={category.icon} />
        <h3 className="mt-4 text-sm font-semibold leading-7 text-zinc-900 dark:text-white">
          <Link href={category.href}>
            <span className="absolute inset-0 rounded-2xl" />
            {category.name}
          </Link>
        </h3>
        <span className="block text-xs text-zinc-500">
            {`${category.packages} Packages`}
        </span>
        <p className="mt-1 text-sm text-zinc-600 dark:text-zinc-400">
          {category.description}
        </p>
      </div>
    </div>
  )
}

export function Categories() {
  return (
    <div className="my-16 xl:max-w-none">
      <Heading level={2} id="categories">
        Pupular Categories
      </Heading>
      <div className="not-prose mt-4 grid grid-cols-1 gap-8 border-t border-zinc-900/5 pt-10 dark:border-white/5 sm:grid-cols-2 xl:grid-cols-4">
        {categories.map((category) => (
          <Category key={category.href} category={category} />
        ))}
        {categories.map((category) => (
          <Category key={category.href} category={category} />
        ))}
      </div>
    </div>
  )
}
