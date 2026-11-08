import type {ReactNode} from 'react';
import clsx from 'clsx';
import Heading from '@theme/Heading';
import styles from './styles.module.css';

type FeatureItem = {
  title: string;
  description: string;
};

const FeatureList: FeatureItem[] = [
  {
    title: '静态类型',
    description:
      '编译时类型检查，在代码运行前捕获类型错误。支持 int、float、bool、string 等基本类型，以及 list、map、array 等容器类型。',
  },
  {
    title: 'Python 式语法',
    description:
      '采用缩进定义代码块，语法简洁易读。无需繁琐的括号和关键字，让代码保持清晰和优雅。',
  },
  {
    title: 'LLVM 原生编译',
    description:
      '基于 LLVM 编译器基础设施，将 Mas lang 代码编译为高效的原生二进制文件。支持 Windows、Linux、macOS 多平台。',
  },
];

function Feature({title, description}: FeatureItem) {
  return (
    <div className={clsx('col col--4')}>
      <div className="text--center padding-horiz--md">
        <Heading as="h3">{title}</Heading>
        <p>{description}</p>
      </div>
    </div>
  );
}

export default function HomepageFeatures(): ReactNode {
  return (
    <section className={styles.features}>
      <div className="container">
        <div className="row">
          {FeatureList.map((props, idx) => (
            <Feature key={idx} {...props} />
          ))}
        </div>
      </div>
    </section>
  );
}
