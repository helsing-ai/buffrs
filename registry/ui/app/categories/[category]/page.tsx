const categories = ["aerospace", "finance", "computer-vision", "science"];

export async function generateStaticParams() {
  return categories.map((category) => ({ category }))
}

const Category = ({ params }: { params: { category: string } }): React.Element => {
  return (
    <>
      <div className="py-8">
        <h1 className="font-bold text-4xl">{params.category}</h1>
      </div>
    </>
  )
}

export default Category;