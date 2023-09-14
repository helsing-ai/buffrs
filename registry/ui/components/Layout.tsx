'use client'

import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { motion } from 'framer-motion'

import { Footer } from '@/components/Footer'
import { Header } from '@/components/Header'
import { Logo } from '@/components/Logo'

export function Layout({
  children,
}: {
  children: React.ReactNode
}) {
  let pathname = usePathname()

  return (
    <div className="h-full">
      <motion.header
        layoutScroll
        className="contents lg:pointer-events-none lg:fixed lg:inset-0 lg:z-40 lg:flex"
      >
        <Header className="lg:pointer-events-auto" />
      </motion.header>
      <div className="relative flex flex-col px-4 pt-14 sm:px-6 lg:px-8">
        <main className="flex-auto">
          <div className="h-full w-full mx-auto h-full w-full max-w-2xl lg:max-w-5xl">
            {children}
          </div>
        </main>
        <Footer />
      </div>
    </div>
  )
}
