import { Button } from '@/components/Button'
import { Hero } from '@/components/hero/Hero'

import { Tag } from '@/components/Tag'

const Package = ({ params }: { params: { id: string } }): React.Element => {
  return (
    <>
      <Hero title={params.id}>
        <div className="">
          <p className="block w-full text-zinc-400">
            This is a very neat description of the package
          </p>

          <div className="mt-4 flex space-x-4">
            <Tag color="rose" className="flex items-center px-3 rounded-[99px]">
              API
            </Tag>

            <Button variant="outline">
              <span className="opacity-50 mr-1">@</span> v1.0.0
            </Button>

            <Button variant="outline">
              <span className="opacity-50 mr-1">owned by</span> @helsing-ai
            </Button>
          </div>
        </div>
      </Hero>
    </>
  )
}

export default Package;