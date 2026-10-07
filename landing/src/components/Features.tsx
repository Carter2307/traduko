import { Languages, ShieldCheck, SpellCheck } from 'lucide-react'
import translationScreenshot from '../assets/traduko-light-cropped.png'

const benefits = [
  {
    icon: ShieldCheck,
    title: 'Your words stay yours.',
    description: 'Translation runs on your Mac. Download your models once, then work offline.',
  },
  {
    icon: Languages,
    title: 'More ways to say hello.',
    description: 'French and English to start. Add more languages whenever you need them.',
  },
  {
    icon: SpellCheck,
    title: 'Color or colour?',
    description: 'American or British English. Choose the spelling that feels like you.',
  },
]

export default function Features() {
  return (
    <>
      <section id="features" className="section-shell scroll-mt-28 pb-16 pt-12 sm:pb-24 md:pt-24" aria-labelledby="features-heading">
        <h2 id="features-heading" className="section-heading reveal">Get to know Traduko.</h2>

        <div className="feature-showcase">
          <article className="feature-card reveal">
            <div className="feature-card-copy">
              <p className="feature-card-eyebrow">Everyday translation</p>
              <h3 className="feature-card-heading">Always a click away.</h3>
              <p className="feature-card-description">
                Type or paste. Traduko translates beside your work, ready whenever you need it.
              </p>
            </div>
            <div className="feature-card-visual">
              <img
                src={translationScreenshot}
                alt="Traduko’s native translation panel, with French input and its English translation."
                width="808"
                height="1096"
                loading="lazy"
                className="feature-card-screenshot [border-radius:8.4%/6.2%]"
              />
            </div>
          </article>

          <article className="feature-card reveal">
            <div className="feature-card-copy">
              <p className="feature-card-eyebrow">A warm welcome</p>
              <h3 className="feature-card-heading">At home from day one.</h3>
              <p className="feature-card-description">
                Meet your companion, choose a model, and settle in. A warm welcome to your Mac.
              </p>
            </div>
            <div className="feature-card-visual">
              <img
                src="/screenshots/01-welcome-cropped.png"
                alt="Traduko’s native welcome screen introducing local translation and its desktop companion."
                width="880"
                height="1280"
                loading="lazy"
                className="feature-card-screenshot [border-radius:7.73%/5.31%]"
              />
            </div>
          </article>
        </div>

        <dl className="feature-benefits">
          {benefits.map(({ icon: Icon, title, description }) => (
            <div key={title} className="benefit-card reveal">
              <dt className="text-[23px] font-semibold leading-[1.2] tracking-[-0.02em] text-[#252422]">
                <Icon size={25} strokeWidth={1.7} className="mb-5 text-traduko" aria-hidden="true" />
                {title}
              </dt>
              <dd className="mt-3 text-[15px] leading-relaxed text-[#65656b]">{description}</dd>
            </div>
          ))}
        </dl>
      </section>
    </>
  )
}
