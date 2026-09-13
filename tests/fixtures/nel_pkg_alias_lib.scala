package nelpkg {
  package data {
    trait Base

    object NonEmptyLazyList {
      type Type[+A] <: Base
    }

    abstract class VersionSpecific {
      type NonEmptyLazyList[+A] = NonEmptyLazyList.Type[A]
    }
  }

  package object data extends data.VersionSpecific
}
