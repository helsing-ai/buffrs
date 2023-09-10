import { Hero } from '@/components/hero/Hero'
import { Categories as Cards } from '@/components/Categories'

const Categories = (): React.Element => {
  return (
    <>
      <Hero title="Categories" />

      <Cards />
    </>
  )
}

export default Categories;